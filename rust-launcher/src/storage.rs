use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{local_app_dir, Persisted};

pub fn state_path() -> PathBuf {
    crate::model::app_dir()
        .join("rust-launcher")
        .join("state.json")
}

pub fn cache_path() -> PathBuf {
    local_app_dir().join("validation-cache-rust.json")
}

pub fn load_state() -> Persisted {
    let path = state_path();
    let backup = path.with_extension("json.bak");

    for candidate in [&path, &backup] {
        let Ok(text) = fs::read_to_string(candidate) else {
            continue;
        };
        let text = text.trim_start_matches('\u{feff}');

        if let Ok(state) = serde_json::from_str::<Persisted>(text) {
            return state;
        }

        if let Some(state) = recover_state(text) {
            tracing::warn!(
                file = %candidate.display(),
                "state.json was partially broken, recovered valid fields"
            );
            return state;
        }
    }

    Persisted::default()
}

/// Rebuilds as much state as possible when the JSON is valid but some fields
/// have the wrong type. Valid fields are kept, broken ones fall back to
/// defaults instead of resetting the whole file.
fn recover_state(text: &str) -> Option<Persisted> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut state = Persisted::default();

    if let Some(settings) = value.get("settings") {
        state.settings = merge_settings(settings);
    }
    if let Some(host) = value.get("serverHost").and_then(|value| value.as_str()) {
        if !host.trim().is_empty() {
            state.server_host = host.to_string();
        }
    }
    if let Some(user) = value.get("user").cloned() {
        if let Ok(user) = serde_json::from_value(user) {
            state.user = Some(user);
        }
    }
    if let Some(saved) = value.get("savedLogin").cloned() {
        if let Ok(saved) = serde_json::from_value(saved) {
            state.saved_login = Some(saved);
        }
    }
    if let Some(profile) = value.get("lastProfile").and_then(|value| value.as_str()) {
        state.last_profile = Some(profile.to_string());
    }
    if let Some(items) = value.get("seenNews").and_then(|value| value.as_array()) {
        state.seen_news = items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .take(200)
            .collect();
    }

    Some(state)
}

fn merge_settings(raw: &serde_json::Value) -> crate::model::Settings {
    let defaults = crate::model::Settings::default();
    let Ok(mut merged) = serde_json::to_value(&defaults) else {
        return defaults;
    };

    if let (Some(merged), Some(raw)) = (merged.as_object_mut(), raw.as_object()) {
        for (key, value) in raw {
            let mut candidate = merged.clone();
            candidate.insert(key.clone(), value.clone());
            if serde_json::from_value::<crate::model::Settings>(serde_json::Value::Object(
                candidate.clone(),
            ))
            .is_ok()
            {
                *merged = candidate;
            }
        }
    }

    serde_json::from_value(merged).unwrap_or(defaults)
}

pub fn save_state(state: &Persisted) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string_pretty(state) {
        let temporary = path.with_extension("json.tmp");
        let backup = path.with_extension("json.bak");

        // Keep the previous good copy so a crash between remove and rename
        // still leaves a recoverable file.
        if path.exists() {
            let _ = fs::copy(&path, &backup);
        }

        if fs::write(&temporary, json).is_ok() {
            let _ = fs::remove_file(&path);
            let _ = fs::rename(temporary, path);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub size: i64,
    pub mtime: i64,
    pub hash: String,
    /// Expected hash from the manifest the cached local hash was checked
    /// against. If the server changes the file, this forces a re-hash instead
    /// of trusting a cache entry that was valid for the previous manifest.
    #[serde(default)]
    pub server_hash: String,
}

pub type ValidationCache = HashMap<String, CacheEntry>;

pub fn cache_key(path: &str) -> String {
    path.replace('/', "\\")
        .trim_start_matches('\\')
        .to_lowercase()
}

pub fn load_cache() -> ValidationCache {
    if let Some(cache) = fs::read_to_string(cache_path())
        .ok()
        .and_then(|text| serde_json::from_str::<ValidationCache>(&text).ok())
    {
        if !cache.is_empty() {
            return cache;
        }
    }

    load_legacy_cache().unwrap_or_default()
}

/// Reuses the SHA-1 cache of the C# launcher so the first Rust launch does not
/// re-hash an entire 16k-file client. Ticks are converted to Unix seconds.
fn load_legacy_cache() -> Option<ValidationCache> {
    #[derive(Deserialize)]
    struct LegacyEntry {
        #[serde(rename = "Size")]
        size: i64,
        #[serde(rename = "LastWriteUtcTicks")]
        ticks: i64,
        #[serde(rename = "Hash")]
        hash: String,
    }

    let path = local_app_dir()
        .join("Gml.Launcher")
        .join("validation-cache-v1.json");
    let text = fs::read_to_string(path).ok()?;
    let legacy: HashMap<String, LegacyEntry> = serde_json::from_str(&text).ok()?;

    const EPOCH_TICKS: i64 = 621_355_968_000_000_000;

    Some(
        legacy
            .into_iter()
            .map(|(key, entry)| {
                (
                    cache_key(&key),
                    CacheEntry {
                        size: entry.size,
                        mtime: (entry.ticks - EPOCH_TICKS) / 10_000_000,
                        hash: entry.hash,
                        server_hash: String::new(),
                    },
                )
            })
            .collect(),
    )
}

pub fn save_cache(cache: &ValidationCache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string(cache) {
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, json).is_ok() {
            let _ = fs::remove_file(&path);
            let _ = fs::rename(temporary, path);
        }
    }
}

/// Turns an API path into a safe path below `root`.
/// Removes stale or excess files from a cache directory. Read-only and
/// best-effort: a locked or malformed entry must never break startup.
pub fn cleanup_cache_dir(dir: &Path, max_age: std::time::Duration, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let now = std::time::SystemTime::now();
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();

    files.sort_by_key(|(_, modified)| *modified);

    // Oldest first: drop everything past the file cap and anything expired.
    let excess = files.len().saturating_sub(max_files);
    for (index, (path, modified)) in files.into_iter().enumerate() {
        let expired = now
            .duration_since(modified)
            .map(|age| age > max_age)
            .unwrap_or(false);
        if index < excess || expired {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn resolve_below(root: &Path, relative: &str) -> Option<PathBuf> {
    let normalized = relative.replace('\\', "/");
    let mut result = root.to_path_buf();

    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        result.push(part);
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_settings_keeps_valid_fields() {
        let raw = serde_json::json!({
            "theme": "ocean",
            "xmx": "not-a-number",
            "sounds": false
        });
        let settings = merge_settings(&raw);
        assert_eq!(settings.theme, "ocean");
        assert!(!settings.sounds);
        assert_eq!(settings.xmx, crate::model::Settings::default().xmx);
    }

    #[test]
    fn recovery_keeps_top_level_fields() {
        let text = r#"{
            "serverHost": "https://msk1.gmlfr.vestalis.xyz/",
            "lastProfile": "Surv10",
            "seenNews": ["a|b", 5, "c|d"],
            "settings": { "theme": "femboy", "xms": "bad" }
        }"#;
        let state = recover_state(text).expect("valid JSON");
        assert_eq!(state.server_host, "https://msk1.gmlfr.vestalis.xyz/");
        assert_eq!(state.last_profile.as_deref(), Some("Surv10"));
        assert_eq!(state.seen_news, vec!["a|b".to_string(), "c|d".to_string()]);
        assert_eq!(state.settings.theme, "femboy");
        assert_eq!(state.settings.xms, crate::model::Settings::default().xms);
    }

    #[test]
    fn resolve_below_rejects_traversal() {
        let root = Path::new(r"C:\root");
        assert!(resolve_below(root, "../evil").is_none());
        assert!(resolve_below(root, "C:/evil").is_none());
        assert_eq!(
            resolve_below(root, "mods/test.jar").unwrap(),
            root.join("mods").join("test.jar")
        );
    }
}
