use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use futures::stream::{self, StreamExt};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::api::Backend;
use crate::model::{ProfileFile, ProfileInfo};
use crate::storage::{
    cache_key, load_cache, resolve_below, save_cache, CacheEntry, ValidationCache,
};

pub struct SyncProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub speed_bps: u64,
    pub current: String,
    /// Bytes transferred in the current phase, when a byte-based progress bar
    /// makes more sense than a file counter (single mod download).
    pub bytes: u64,
    pub total_bytes: u64,
    /// Remaining time estimate for the current phase, e.g. "2:14".
    pub eta: String,
}

pub type ProgressFn = Arc<dyn Fn(SyncProgress) + Send + Sync>;

#[derive(Clone)]
struct DownloadTask {
    file: ProfileFile,
    target: PathBuf,
}

struct ProgressState {
    total: usize,
    total_bytes: u64,
    done: AtomicUsize,
    bytes: AtomicU64,
    start: Instant,
    last_emit: Mutex<Instant>,
    phase: String,
}

impl ProgressState {
    fn new(total: usize, phase: &str) -> Self {
        Self::with_bytes(total, 0, phase)
    }

    fn with_bytes(total: usize, total_bytes: u64, phase: &str) -> Self {
        Self {
            total,
            total_bytes,
            done: AtomicUsize::new(0),
            bytes: AtomicU64::new(0),
            start: Instant::now(),
            last_emit: Mutex::new(Instant::now() - Duration::from_secs(1)),
            phase: phase.to_string(),
        }
    }

    fn emit(&self, callback: &ProgressFn, current: &str, force: bool) {
        let done = self.done.load(Ordering::Relaxed);
        let mut last = self.last_emit.lock().unwrap();
        if !force && last.elapsed() < Duration::from_millis(120) {
            return;
        }
        *last = Instant::now();

        let bytes = self.bytes.load(Ordering::Relaxed);
        let seconds = self.start.elapsed().as_secs_f64().max(0.001);
        let eta = if self.total_bytes > 0 && bytes > 0 && bytes < self.total_bytes {
            let remaining = (self.total_bytes - bytes) as f64 / (bytes as f64 / seconds);
            format_eta(remaining)
        } else if done > 0 && done < self.total {
            let remaining = (self.total - done) as f64 * (seconds / done as f64);
            format_eta(remaining)
        } else {
            String::new()
        };

        callback(SyncProgress {
            phase: self.phase.clone(),
            done,
            total: self.total,
            speed_bps: (bytes as f64 / seconds) as u64,
            current: current.to_string(),
            bytes,
            total_bytes: self.total_bytes,
            eta,
        });
    }
}

fn format_eta(seconds: f64) -> String {
    let seconds = seconds.clamp(0.0, 86_400.0) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// Downloads a single optional mod into `target` with retries. The caller
/// chooses the `.disabled` path when the mod should stay off after download.
pub async fn download_optional(
    backend: &Backend,
    hash: &str,
    target: &Path,
    expected_size: u64,
    name: &str,
    progress: ProgressFn,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let state = Arc::new(ProgressState::with_bytes(1, expected_size, "Загрузка мода"));
    let mut last_error = None;

    for attempt in 1..=3 {
        match download_once(backend, hash, target, &state, &progress, name, None).await {
            Ok(()) => {
                state.done.store(1, Ordering::Relaxed);
                state.emit(&progress, name, true);
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Не удалось скачать мод {name}")))
}

/// Validates every manifest entry and downloads the missing or changed files.
/// Files larger than 100 MiB are trusted by size, exactly like the reference client:
/// hashing a whole client jar on every launch would dominate startup time.
pub async fn sync_profile(
    backend: &Backend,
    info: &ProfileInfo,
    root: &Path,
    progress: ProgressFn,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let total = info.files.len();
    let state = Arc::new(ProgressState::new(total, "Этап 1/3 · Проверка файлов"));
    let cache = Arc::new(Mutex::new(load_cache()));

    let candidate_whitelist = Arc::new(info.white_list_files.clone());
    let scanned = Arc::new(AtomicUsize::new(0));
    let mut stream = stream::iter(info.files.iter().cloned())
        .map(|file| {
            let root = root.to_path_buf();
            let cache = cache.clone();
            let state = state.clone();
            let progress = progress.clone();
            let scanned = scanned.clone();
            let white_list = candidate_whitelist.clone();
            let cancel = cancel.clone();

            async move {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }

                let task = tokio::task::spawn_blocking(move || {
                    validate_one(&root, &file, &cache, &white_list)
                })
                .await
                .ok()
                .flatten();

                let done = scanned.fetch_add(1, Ordering::Relaxed) + 1;
                state.done.store(done, Ordering::Relaxed);
                state.emit(
                    &progress,
                    "Проверка целостности",
                    done == total || done % 200 == 0,
                );

                task
            }
        })
        .buffer_unordered(16);

    let mut tasks: Vec<DownloadTask> = Vec::new();
    while let Some(task) = stream.next().await {
        if let Some(task) = task {
            tasks.push(task);
        }
    }

    {
        let cache = cache.lock().unwrap();
        save_cache(&cache);
    }

    info!(total, to_update = tasks.len(), "profile files validated");

    if cancel.load(Ordering::Relaxed) {
        return Err(anyhow!("Отменено"));
    }

    if tasks.is_empty() {
        state.done.store(total, Ordering::Relaxed);
        state.emit(&progress, "Этап 2/3 · Файлы актуальны", true);
        return Ok(());
    }

    for task in tasks.iter().take(12) {
        info!(file = %task.file.directory, "file will be updated");
    }

    let files_total = tasks.len();
    let download_state = Arc::new(ProgressState::new(
        files_total,
        "Этап 2/3 · Загрузка файлов",
    ));
    download_state.done.store(0, Ordering::Relaxed);

    let mut downloads = stream::iter(tasks)
        .map(|task| {
            let backend = backend.clone();
            let state = download_state.clone();
            let progress = progress.clone();
            let cache = cache.clone();
            let cancel = cancel.clone();

            async move {
                if cancel.load(Ordering::Relaxed) {
                    return Err(anyhow!("Отменено"));
                }
                let name = task.file.name.clone();
                let result =
                    download_one(&backend, &task, &state, &progress, &cache, Some(&cancel)).await;
                if result.is_ok() {
                    state.done.fetch_add(1, Ordering::Relaxed);
                    state.emit(&progress, &name, true);
                }
                result
            }
        })
        .buffer_unordered(32);

    let mut first_error: Option<anyhow::Error> = None;
    while let Some(result) = downloads.next().await {
        if let Err(error) = result {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }

    if let Some(error) = first_error {
        return Err(error);
    }

    {
        let cache = cache.lock().unwrap();
        save_cache(&cache);
    }

    download_state.emit(&progress, "Этап 3/3 · Подготовка завершена", true);
    Ok(())
}

fn validate_one(
    root: &Path,
    file: &ProfileFile,
    cache: &Arc<Mutex<ValidationCache>>,
    white_list: &[ProfileFile],
) -> Option<DownloadTask> {
    let target = resolve_below(root, &file.directory)?;
    let optional = file.directory.contains("-optional-mod.jar");

    let target = if optional && !target.exists() {
        PathBuf::from(format!("{}.disabled", target.display()))
    } else {
        target
    };

    let metadata = std::fs::metadata(&target).ok();

    if let Some(metadata) = metadata {
        let size = metadata.len() as i64;

        // User-owned files (options, configs, saves, logs) must survive updates.
        // This is the same containment check the reference client uses.
        let whitelisted = white_list
            .iter()
            .any(|allowed| allowed.directory.contains(&file.directory));
        if whitelisted {
            return None;
        }

        // The size in the manifest is authoritative. A mismatch means the
        // server replaced the file even if the hash is missing or stale.
        if file.size > 0 && size != file.size {
            return Some(DownloadTask {
                file: file.clone(),
                target,
            });
        }

        // Huge files are trusted by presence; hashing them every launch is pointless.
        if size >= 100 * 1024 * 1024 {
            return None;
        }

        if let Some(expected) = file.hash.as_deref().filter(|hash| !hash.is_empty()) {
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0);

            let key = cache_key(&file.directory);
            let cached = {
                let cache = cache.lock().unwrap();
                cache.get(&key).cloned()
            };

            // Reuse the cached hash only while the local file is unchanged AND
            // the manifest still expects the same hash. After a server update
            // the recorded server hash differs, forcing a real re-hash.
            let hash = match cached {
                Some(entry)
                    if entry.size == size
                        && entry.mtime == mtime
                        && (entry.server_hash.is_empty()
                            || entry.server_hash.eq_ignore_ascii_case(expected)) =>
                {
                    entry.hash
                }
                _ => match hash_file(&target) {
                    Ok(hash) => {
                        let mut cache = cache.lock().unwrap();
                        cache.insert(
                            key,
                            CacheEntry {
                                size,
                                mtime,
                                hash: hash.clone(),
                                server_hash: expected.to_string(),
                            },
                        );
                        hash
                    }
                    Err(_) => String::new(),
                },
            };

            if hash.eq_ignore_ascii_case(expected) {
                return None;
            }
        }
    }

    Some(DownloadTask {
        file: file.clone(),
        target,
    })
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 1024 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

async fn download_one(
    backend: &Backend,
    task: &DownloadTask,
    state: &Arc<ProgressState>,
    progress: &ProgressFn,
    cache: &Arc<Mutex<ValidationCache>>,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    let hash = task
        .file
        .hash
        .as_deref()
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| anyhow!("В манифесте нет хэша для {}", task.file.directory))?;

    if let Some(parent) = task.target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let mut last_error = None;

    for attempt in 1..=3 {
        if cancel
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            return Err(anyhow!("Отменено"));
        }
        match download_once(
            backend,
            hash,
            &task.target,
            state,
            progress,
            &task.file.name,
            cancel,
        )
        .await
        {
            Ok(()) => {
                // Record the freshly downloaded file so the next launch does not
                // hash it again. The server hash is stored too, so a later
                // manifest change still invalidates the entry.
                if let Ok(metadata) = tokio::fs::metadata(&task.target).await {
                    let size = metadata.len() as i64;
                    let mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_secs() as i64)
                        .unwrap_or(0);
                    let key = cache_key(&task.file.directory);

                    if let Ok(mut cache) = cache.lock() {
                        cache.insert(
                            key,
                            CacheEntry {
                                size,
                                mtime,
                                hash: hash.to_lowercase(),
                                server_hash: hash.to_lowercase(),
                            },
                        );
                    }
                }

                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Не удалось скачать {}", task.file.directory)))
}

async fn download_once(
    backend: &Backend,
    hash: &str,
    target: &Path,
    state: &Arc<ProgressState>,
    progress: &ProgressFn,
    current: &str,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    let url = backend.file_url(hash);
    let request = backend.client().get(&url).timeout(Duration::from_secs(600));
    let response = backend
        .sign(request, backend.client_hwid())
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;

    let temporary = target.with_extension(format!(
        "{}tmp",
        target
            .extension()
            .map(|extension| format!("{}.", extension.to_string_lossy()))
            .unwrap_or_default()
    ));

    let mut file = tokio::fs::File::create(&temporary)
        .await
        .with_context(|| format!("создание {}", temporary.display()))?;

    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if cancel
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(anyhow!("Отменено"));
        }
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        state.bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        state.emit(progress, current, false);
    }

    file.flush().await?;
    drop(file);

    // Zero-byte files are legitimate: the manifest contains placeholders such
    // as the empty-file SHA-1 (da39a3ee...). Only a failed HTTP request is an error.

    let _ = tokio::fs::remove_file(target).await;
    tokio::fs::rename(&temporary, target).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_entry(directory: &str, size: i64, hash: &str) -> ProfileFile {
        ProfileFile {
            name: directory
                .rsplit('/')
                .next()
                .unwrap_or(directory)
                .to_string(),
            directory: directory.to_string(),
            hash: Some(hash.to_string()),
            size,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("vestalis-test-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    async fn cancelled_sync_stops_before_downloads() {
        let info = ProfileInfo {
            java_path: String::new(),
            relative_path: String::new(),
            arguments: String::new(),
            files: Vec::new(),
            white_list_files: Vec::new(),
        };
        let backend = Backend::new("https://example.invalid/");
        let cancel = Arc::new(AtomicBool::new(true));
        let progress: ProgressFn = Arc::new(|_| {});

        let error = sync_profile(&backend, &info, &temp_root("cancel"), progress, cancel)
            .await
            .expect_err("cancelled sync must fail");

        assert!(error.to_string().contains("Отменено"));
    }

    #[test]
    fn changed_manifest_size_forces_download_without_hashing() {
        let root = temp_root("size");
        let relative = "game data/Surv10/mods/test.jar";
        let target = resolve_below(&root, relative).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old").unwrap();

        // Server reports a different size (e.g. mod was replaced) and a hash
        // that is intentionally not the local one.
        let entry = file_entry(relative, 999, "0000000000000000000000000000000000000000");
        let cache = Arc::new(Mutex::new(ValidationCache::new()));

        let task = validate_one(&root, &entry, &cache, &[]);
        assert!(task.is_some(), "size mismatch must schedule an update");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn matching_size_and_hash_is_skipped() {
        let root = temp_root("same");
        let relative = "game data/Surv10/mods/test.jar";
        let target = resolve_below(&root, relative).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"same-content").unwrap();

        let metadata = std::fs::metadata(&target).unwrap();
        let size = metadata.len() as i64;
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let hash = hash_file(&target).unwrap();

        let entry = file_entry(relative, size, &hash);
        let mut cache = ValidationCache::new();
        cache.insert(
            cache_key(relative),
            CacheEntry {
                size,
                mtime,
                hash: hash.clone(),
                server_hash: hash.clone(),
            },
        );
        let cache = Arc::new(Mutex::new(cache));

        assert!(validate_one(&root, &entry, &cache, &[]).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn changed_manifest_hash_invalidates_cache_entry() {
        let root = temp_root("hash");
        let relative = "game data/Surv10/mods/test.jar";
        let target = resolve_below(&root, relative).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        // Same length as "new-content", so only the hash comparison can catch it.
        std::fs::write(&target, b"old-content").unwrap();

        let metadata = std::fs::metadata(&target).unwrap();
        let size = metadata.len() as i64;
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let old_hash = hash_file(&target).unwrap();
        let new_hash = hex::encode(Sha1::digest(b"new-content"));

        let entry = file_entry(relative, size, &new_hash);
        let mut cache = ValidationCache::new();
        cache.insert(
            cache_key(relative),
            CacheEntry {
                size,
                mtime,
                hash: old_hash.clone(),
                server_hash: old_hash,
            },
        );
        let cache = Arc::new(Mutex::new(cache));

        assert!(validate_one(&root, &entry, &cache, &[]).is_some());

        let _ = std::fs::remove_dir_all(&root);
    }
}
