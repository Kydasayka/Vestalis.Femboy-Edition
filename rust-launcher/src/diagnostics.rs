//! Support bundle: logs, version, sanitized state, themes and layout.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::model::Settings;

const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;

pub fn export(settings: &Settings) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("vestalis-diagnostics-{stamp}.zip");
    let target = export_dir().join(file_name);

    std::fs::create_dir_all(export_dir()).ok();

    let file =
        std::fs::File::create(&target).with_context(|| format!("создание {}", target.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let readme = format!(
        "Vestalis Launcher diagnostics\nversion: {}\nos: {}\narch: {}\ncreated: {}\n\
         telemetry enabled: {}\n\n\
         Tokens, passwords and saved credentials are removed from state.json.\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().to_rfc3339(),
        settings.telemetry_enabled
    );
    write_entry(&mut zip, "README.txt", readme.as_bytes(), options)?;

    if let Some(state) = sanitized_state() {
        write_entry(&mut zip, "state.sanitized.json", state.as_bytes(), options)?;
    }

    let settings_json = serde_json::to_vec_pretty(settings).unwrap_or_default();
    write_entry(&mut zip, "settings.json", &settings_json, options)?;

    let themes: Vec<Value> = crate::theme::load_all()
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "name": item.name,
                "builtin": item.builtin,
                "icons": item.icons.keys().cloned().collect::<Vec<_>>(),
                "texts": item.texts.keys().cloned().collect::<Vec<_>>(),
            })
        })
        .collect();
    let themes_json = serde_json::to_vec_pretty(&themes).unwrap_or_default();
    write_entry(&mut zip, "themes.json", &themes_json, options)?;

    let log_dir = crate::model::local_app_dir().join("logs");
    if let Ok(entries) = std::fs::read_dir(&log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            let data = if data.len() > MAX_LOG_BYTES {
                &data[data.len() - MAX_LOG_BYTES..]
            } else {
                &data
            };
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "launcher.log".into());
            write_entry(&mut zip, &format!("logs/{name}"), data, options)?;
        }
    }

    zip.finish().context("завершение zip")?;
    Ok(target)
}

fn write_entry(
    zip: &mut zip::ZipWriter<std::fs::File>,
    name: &str,
    data: &[u8],
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(data)?;
    Ok(())
}

fn export_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Desktop"))
        .filter(|desktop| desktop.exists())
        .unwrap_or_else(crate::model::local_app_dir)
}

fn sanitized_state() -> Option<String> {
    let text = std::fs::read_to_string(crate::storage::state_path()).ok()?;
    let mut value: Value = serde_json::from_str(&text).ok()?;
    scrub(&mut value);
    serde_json::to_string_pretty(&value).ok()
}

fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| {
                let key = key.to_ascii_lowercase();
                !(key.contains("token")
                    || key.contains("password")
                    || key.contains("secret")
                    || key.contains("credential")
                    || key == "user"
                    || key == "savedlogin")
            });
            for value in map.values_mut() {
                scrub(value);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubbing_removes_credentials_but_keeps_settings() {
        let mut value = serde_json::json!({
            "user": { "accessToken": "secret", "name": "player" },
            "savedLogin": { "login": "a", "password": "b" },
            "settings": { "theme": "dark", "sendHwid": true },
            "nested": { "apiKeySecret": "x", "ok": 1 }
        });

        scrub(&mut value);
        assert!(value.get("user").is_none());
        assert!(value.get("savedLogin").is_none());
        assert_eq!(value["settings"]["theme"], "dark");
        assert!(value["nested"].get("apiKeySecret").is_none());
        assert_eq!(value["nested"]["ok"], 1);
    }
}
