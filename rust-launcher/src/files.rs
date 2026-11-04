use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use futures::stream::{self, StreamExt};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

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
}

pub type ProgressFn = Arc<dyn Fn(SyncProgress) + Send + Sync>;

#[derive(Clone)]
struct DownloadTask {
    file: ProfileFile,
    target: PathBuf,
}

struct ProgressState {
    total: usize,
    done: AtomicUsize,
    bytes: AtomicU64,
    start: Instant,
    last_emit: Mutex<Instant>,
    phase: String,
}

impl ProgressState {
    fn new(total: usize, phase: &str) -> Self {
        Self {
            total,
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

        callback(SyncProgress {
            phase: self.phase.clone(),
            done,
            total: self.total,
            speed_bps: (bytes as f64 / seconds) as u64,
            current: current.to_string(),
        });
    }
}

/// Validates every manifest entry and downloads the missing or changed files.
/// Files larger than 100 MiB are trusted by size, exactly like the C# client:
/// hashing a whole client jar on every launch would dominate startup time.
pub async fn sync_profile(
    backend: &Backend,
    info: &ProfileInfo,
    root: &Path,
    progress: ProgressFn,
) -> Result<()> {
    let total = info.files.len();
    let state = Arc::new(ProgressState::new(total, "Проверка файлов"));
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

            async move {
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

    if tasks.is_empty() {
        state.done.store(total, Ordering::Relaxed);
        state.emit(&progress, "Все файлы актуальны", true);
        return Ok(());
    }

    let files_total = tasks.len();
    let download_state = Arc::new(ProgressState::new(files_total, "Загрузка файлов"));
    download_state.done.store(0, Ordering::Relaxed);

    let mut downloads = stream::iter(tasks)
        .map(|task| {
            let backend = backend.clone();
            let state = download_state.clone();
            let progress = progress.clone();

            async move {
                let name = task.file.name.clone();
                let result = download_one(&backend, &task, &state, &progress).await;
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

    download_state.emit(&progress, "Готово", true);
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
        // Huge files are trusted by presence; hashing them every launch is pointless.
        if metadata.len() >= 100 * 1024 * 1024 {
            return None;
        }

        if let Some(expected) = file.hash.as_deref().filter(|hash| !hash.is_empty()) {
            let size = metadata.len() as i64;
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

            let hash = match cached {
                Some(entry) if entry.size == size && entry.mtime == mtime => entry.hash,
                _ => match hash_file(&target) {
                    Ok(hash) => {
                        let mut cache = cache.lock().unwrap();
                        cache.insert(
                            key,
                            CacheEntry {
                                size,
                                mtime,
                                hash: hash.clone(),
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

            if white_list
                .iter()
                .any(|allowed| allowed.directory.contains(&file.directory))
            {
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
        match download_once(
            backend,
            hash,
            &task.target,
            state,
            progress,
            &task.file.name,
        )
        .await
        {
            Ok(()) => return Ok(()),
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
