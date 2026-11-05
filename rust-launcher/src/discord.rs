use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::model::DiscordConfig;

#[derive(Clone)]
pub struct DiscordRpc {
    tx: Sender<String>,
}

impl DiscordRpc {
    pub fn start(config: DiscordConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || run(rx, config))
            .ok();

        Self { tx }
    }

    pub fn set_state(&self, state: impl Into<String>) {
        let _ = self.tx.send(state.into());
    }
}

fn run(rx: Receiver<String>, config: DiscordConfig) {
    let mut state = String::from("В лаунчере");

    loop {
        let mut client = match DiscordIpcClient::new(&config.client_id) {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "discord rpc client creation failed");
                thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

        if let Err(error) = client.connect() {
            tracing::warn!(%error, "discord rpc connect failed");
            thread::sleep(Duration::from_secs(5));
            continue;
        }

        tracing::info!("discord rpc connected");
        if let Err(error) = set_activity(&mut client, &state, &config) {
            tracing::warn!(%error, "discord rpc activity rejected");
        }

        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(next) => {
                    state = next;
                    if let Err(error) = set_activity(&mut client, &state, &config) {
                        tracing::warn!(%error, "discord rpc activity update failed");
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = client.close();
                    return;
                }
            }
        }

        let _ = client.close();
        thread::sleep(Duration::from_secs(2));
    }
}

fn set_activity(
    client: &mut DiscordIpcClient,
    state: &str,
    config: &DiscordConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let mut presence = activity::Activity::new()
        .details(&config.details)
        .state(state)
        .timestamps(activity::Timestamps::new().start(started));

    if !config.large_image_key.is_empty() {
        let mut assets = activity::Assets::new();
        assets = assets.large_image(&config.large_image_key);
        if !config.large_image_text.is_empty() {
            assets = assets.large_text(&config.large_image_text);
        }
        if !config.small_image_key.is_empty() {
            assets = assets.small_image(&config.small_image_key);
            if !config.small_image_text.is_empty() {
                assets = assets.small_text(&config.small_image_text);
            }
        }
        presence = presence.assets(assets);
    }

    client.set_activity(presence)?;
    Ok(())
}
