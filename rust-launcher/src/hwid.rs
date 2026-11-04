use std::process::Command;
use std::sync::OnceLock;

use uuid::Uuid;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

/// Must match the C# launcher exactly: `Hardware.Info` sends
/// `CPU.ProcessorId-Motherboard.SerialNumber-Disk1-Disk2...`. A different value
/// looks like another device to the backend, which can reject the account.
pub fn real_hwid() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();

    CACHE
        .get_or_init(|| cim_hwid().unwrap_or_else(registry_hwid))
        .clone()
}

fn cim_hwid() -> Option<String> {
    const SCRIPT: &str = "$cpu=(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty ProcessorId); \
$mb=(Get-CimInstance Win32_BaseBoard | Select-Object -First 1 -ExpandProperty SerialNumber); \
$disks=(Get-CimInstance Win32_DiskDrive | ForEach-Object { $_.SerialNumber }) -join '-'; \
\"$cpu-$mb-$disks\"";

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        SCRIPT,
    ]);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() || value == "---" {
        None
    } else {
        Some(value)
    }
}

fn registry_hwid() -> String {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    let machine_guid: String = hklm
        .open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", KEY_READ)
        .and_then(|key| key.get_value("MachineGuid"))
        .unwrap_or_else(|_| "NOT_FOUND".to_string());

    let cpu: String = hklm
        .open_subkey_with_flags(
            "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",
            KEY_READ,
        )
        .and_then(|key| key.get_value("ProcessorNameString"))
        .unwrap_or_else(|_| "NOT_FOUND".to_string());

    format!("{}-{}", cpu.trim(), machine_guid.trim())
}

/// The backend requires `X-HWID` on every auth request. When the user turns the
/// real HWID off we send a fresh random value instead, so a ban bound to one
/// identifier does not stick.
pub fn random_hwid() -> String {
    Uuid::new_v4().to_string().to_uppercase()
}

pub fn total_ram_mb() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    (system.total_memory() / (1024 * 1024)).max(1024)
}

pub fn hwid_for(settings: &crate::model::Settings) -> String {
    if settings.send_hwid {
        real_hwid()
    } else {
        random_hwid()
    }
}

pub fn os_type() -> &'static str {
    "3" // Gml.Web.Api.Domains.System.OsType.Windows
}

pub fn os_architecture() -> &'static str {
    // Must match the C# launcher: OSArchitecture.ToString().ToLower().Replace("x", "").
    // The backend returns an empty/error profile for "x64" but full data for "64".
    if cfg!(target_arch = "x86_64") {
        "64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "86"
    }
}
