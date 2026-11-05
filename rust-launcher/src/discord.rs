use std::fs::File;
use std::io::Write;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use uuid::Uuid;

/// Minimal Discord IPC client (the same presence the C# launcher sets through
/// DiscordRPC). It is intentionally dependency-free: a named pipe, a JSON
/// handshake and SET_ACTIVITY frames.
const CLIENT_ID: &str = "1558556890605162577";
const DETAILS: &str = "Продам гараж с чулочками, по поводу покупки - пишите в ЛС. Myaw >w<";

#[derive(Clone)]
pub struct DiscordRpc {
    tx: Sender<String>,
}

impl DiscordRpc {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || run(rx))
            .ok();

        Self { tx }
    }

    pub fn set_state(&self, state: impl Into<String>) {
        let _ = self.tx.send(state.into());
    }
}

fn run(rx: Receiver<String>) {
    let mut pipe: Option<File> = None;
    let mut state = String::from("В лаунчере");
    let mut reported = false;

    loop {
        if pipe.is_none() {
            match connect() {
                Some(mut file) => {
                    let handshake_ok = handshake(&mut file);
                    let activity_ok = handshake_ok && send_activity(&mut file, &state);
                    if activity_ok {
                        tracing::info!("discord rpc connected");
                        pipe = Some(file);
                    } else if !reported {
                        tracing::warn!(handshake_ok, activity_ok, "discord rpc handshake failed");
                        reported = true;
                    }
                }
                None => {
                    if !reported {
                        tracing::warn!("discord rpc pipe not found");
                        reported = true;
                    }
                }
            }
        }

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(next) => {
                state = next;
                if let Some(file) = pipe.as_mut() {
                    if !send_activity(file, &state) {
                        pipe = None;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut std::ffi::c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: isize,
    ) -> isize;
}

fn connect() -> Option<File> {
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawHandle;

        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const OPEN_EXISTING: u32 = 3;
        const INVALID_HANDLE_VALUE: isize = -1;

        for index in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{index}");
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    0,
                )
            };

            if handle != INVALID_HANDLE_VALUE {
                return Some(unsafe { File::from_raw_handle(handle as *mut _) });
            }
        }
    }

    None
}

fn handshake(pipe: &mut File) -> bool {
    let value = json!({
        "v": 1,
        "client_id": CLIENT_ID,
    });
    send_frame(pipe, 0, &value)
}

fn send_activity(pipe: &mut File, state: &str) -> bool {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);

    let activity = json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": {
                "details": DETAILS,
                "state": state,
                "assets": {
                    "large_image": "logo2",
                    "large_text": "~Nya~",
                    "small_image": "logo",
                    "small_text": "Vestalis FemBoy Edition"
                },
                "timestamps": { "start": started }
            }
        },
        "nonce": Uuid::new_v4().to_string()
    });

    send_frame(pipe, 1, &activity)
}

fn send_frame(pipe: &mut File, opcode: u32, value: &serde_json::Value) -> bool {
    let payload = value.to_string();
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload.as_bytes());

    pipe.write_all(&frame).is_ok() && pipe.flush().is_ok()
}
