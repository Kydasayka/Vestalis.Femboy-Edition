//! Minimal SignalR (JSON protocol) client for the launcher hub.
//!
//! The C# client connects to `wss://<host>/ws/launcher?access_token=...` and
//! listens for `RefreshProfiles`. Reimplementing the whole SignalR stack is not
//! necessary here: negotiation, the JSON handshake, invocations and pings are
//! enough for the events the launcher consumes.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

const RECORD_SEPARATOR: char = '\u{1e}';

/// Events delivered from the hub to the worker.
#[derive(Debug, Clone, Copy)]
pub enum HubEvent {
    RefreshProfiles,
}

/// Runs the hub connection forever, reconnecting with exponential backoff.
pub async fn run(base: String, token: String, user: String, tx: UnboundedSender<HubEvent>) {
    let mut delay = 1u64;

    loop {
        match connection(&base, &token, &user, &tx).await {
            Ok(()) => {
                info!("hub connection finished");
                delay = 1;
            }
            Err(error) => {
                warn!(%error, "hub connection failed");
            }
        }

        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(30);
    }
}

async fn connection(
    base: &str,
    token: &str,
    user: &str,
    tx: &UnboundedSender<HubEvent>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let (host, ws_scheme) = split_host(base);
    let negotiate = format!(
        "https://{host}/ws/launcher/negotiate?negotiateVersion=1&access_token={}",
        urlencode(token)
    );

    let response = client.post(&negotiate).send().await?;
    if !response.status().is_success() {
        bail!(
            "hub negotiation returned HTTP {}",
            response.status().as_u16()
        );
    }

    let payload: Value = response.json().await?;
    let connection_token = payload
        .get("connectionToken")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("hub negotiation has no connection token"))?;

    let url = format!(
        "{ws_scheme}://{host}/ws/launcher?id={}&access_token={}",
        urlencode(connection_token),
        urlencode(token)
    );

    let (mut socket, _) = tokio_tungstenite::connect_async(url).await?;

    socket
        .send(Message::text(
            r#"{"protocol":"json","version":1}"#.to_string() + &RECORD_SEPARATOR.to_string(),
        ))
        .await?;

    // The handshake response is an empty JSON object; consume it before
    // listening for invocations.
    match socket.next().await {
        Some(Ok(Message::Text(text))) => {
            if !text.trim().eq("{}") && !text.trim().is_empty() {
                debug!(%text, "unexpected hub handshake response");
            }
        }
        Some(Ok(Message::Close(_))) | None => bail!("hub closed during handshake"),
        Some(Err(error)) => return Err(error.into()),
        _ => {}
    }

    info!("hub connected");
    if let Some(reply) = invocation("UpdateUserLauncher", serde_json::json!([user])) {
        socket.send(Message::text(reply)).await?;
    }

    let mut ping = tokio::time::interval(Duration::from_secs(15));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        for reply in handle_text(&text, tx) {
                            socket.send(Message::text(reply)).await?;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        socket.send(Message::Pong(payload)).await?;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        debug!("hub closed");
                        return Ok(());
                    }
                    Some(Err(error)) => return Err(error.into()),
                    _ => {}
                }
            }
            _ = ping.tick() => {
                // SignalR keep-alive; the server drops idle connections.
                socket
                    .send(Message::text(
                        r#"{"type":6}"#.to_string() + &RECORD_SEPARATOR.to_string(),
                    ))
                    .await?;
            }
        }
    }
}

fn handle_text(text: &str, tx: &UnboundedSender<HubEvent>) -> Vec<String> {
    let mut replies = Vec::new();

    for frame in text.split(RECORD_SEPARATOR) {
        let frame = frame.trim();
        if frame.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };

        let message_type = value.get("type").and_then(Value::as_u64).unwrap_or(0);
        if message_type != 1 {
            continue;
        }

        match value.get("target").and_then(Value::as_str).unwrap_or("") {
            "RefreshProfiles" => {
                debug!("hub requested a profile refresh");
                let _ = tx.send(HubEvent::RefreshProfiles);
            }
            "RequestLauncherHash" => {
                if let Some(reply) = invocation("ConfirmLauncherHash", serde_json::json!(["hash"]))
                {
                    replies.push(reply);
                }
            }
            _ => {}
        }
    }

    replies
}

fn invocation(target: &str, arguments: Value) -> Option<String> {
    Some(
        serde_json::json!({
            "type": 1,
            "target": target,
            "arguments": arguments,
        })
        .to_string()
            + &RECORD_SEPARATOR.to_string(),
    )
}

/// Splits a backend base URL into `host[:port]` and the websocket scheme.
fn split_host(base: &str) -> (&str, &str) {
    let trimmed = base.trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("https://") {
        (rest, "wss")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        (rest, "ws")
    } else {
        (trimmed, "wss")
    }
}

fn urlencode(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char)
            }
            _ => result.push_str(&format!("%{byte:02X}")),
        }
    }
    result
}

use tokio_tungstenite::tungstenite::Message;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_https_and_http_hosts() {
        assert_eq!(
            split_host("https://msk1.gmlfr.vestalis.xyz/"),
            ("msk1.gmlfr.vestalis.xyz", "wss")
        );
        assert_eq!(
            split_host("http://127.0.0.1:8080"),
            ("127.0.0.1:8080", "ws")
        );
    }

    #[test]
    fn refresh_profiles_emits_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let frames = concat!(
            r#"{"type":1,"target":"RefreshProfiles","arguments":[]}"#,
            "\u{1e}"
        );
        assert!(handle_text(frames, &tx).is_empty());
        assert!(matches!(rx.try_recv(), Ok(HubEvent::RefreshProfiles)));
    }

    #[test]
    fn launcher_hash_request_gets_confirmation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let frames = concat!(
            r#"{"type":1,"target":"RequestLauncherHash","arguments":[]}"#,
            "\u{1e}"
        );
        let replies = handle_text(frames, &tx);
        assert_eq!(replies.len(), 1);
        assert!(replies[0].contains("ConfirmLauncherHash"));
        assert!(replies[0].ends_with(RECORD_SEPARATOR));
    }

    #[test]
    fn unknown_targets_are_ignored() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let frames = concat!(
            r#"{"type":1,"target":"SomethingElse","arguments":[]}"#,
            "\u{1e}"
        );
        assert!(handle_text(frames, &tx).is_empty());
    }
}
