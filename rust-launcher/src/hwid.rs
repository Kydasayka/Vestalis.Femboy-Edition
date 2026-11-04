use uuid::Uuid;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

/// A stable string that identifies the machine to the backend. It mirrors the
/// CPU / motherboard / disk idea of the C# launcher without pulling a hardware
/// inventory crate: the Windows MachineGuid plus the CPU model string is stable
/// across reboots and unique enough for HWID bans.
pub fn real_hwid() -> String {
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
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86"
    }
}
