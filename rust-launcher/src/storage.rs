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
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_state(state: &Persisted) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string_pretty(state) {
        let temporary = path.with_extension("json.tmp");
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
