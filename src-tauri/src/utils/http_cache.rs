// Disk-backed HTTP ETag cache.
//
// On disk: `<dir>/<sha256(url)>.meta.json` + `<dir>/<sha256(url)>.body`.
// 304 responses do not count against GitHub API rate limits, so revalidation
// is effectively free. Stale cache is served when the network is down.

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tauri::path::BaseDirectory;
use tokio::sync::Mutex;

use crate::consts::BASE_DIR;

/// Max total disk usage for http_cache (10 MB).
const MAX_CACHE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Serializes all cache I/O. `fetch` holds this across the network await so
/// that concurrent calls for the same URL coalesce into one request instead of
/// racing on the meta/body files (and so eviction cannot delete a body that
/// another call is about to read). The lock is global — cache calls are few
/// and short, so this is cheaper than per-key locking.
static FETCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn fetch_lock() -> &'static Mutex<()> {
    FETCH_LOCK.get_or_init(|| Mutex::new(()))
}

/// Shared client for ad-hoc metadata GETs/HEADs (release index, manifests,
/// bg etag). One connection pool instead of a fresh TLS context per call
/// site — `reqwest::Client::new()` was created on the fly in 6 places.
pub static SHARED_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheSource {
    /// Served from cache because TTL was not expired (no request sent).
    Fresh,
    /// Server returned 304 Not Modified (body came from disk).
    Revalidated,
    /// Network error — returned stale cache entry with a warning.
    StaleFallback,
    /// Server returned 200 with a new body (cache was updated).
    Updated,
}

#[derive(Debug)]
pub struct CachedBody {
    pub bytes: Vec<u8>,
    pub source: CacheSource,
}

// ---------------------------------------------------------------------------
// Internal metadata
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct CacheMeta {
    etag: Option<String>,
    fetched_at: String, // ISO-8601 UTC
    url: String,
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Call once during `tauri_setup`, before any async work.
pub fn init(app_handle: &tauri::AppHandle) -> Result<()> {
    let dir = app_handle
        .path()
        .resolve(BASE_DIR, BaseDirectory::AppConfig)
        .context("Failed to resolve AppConfig path for http_cache")?
        .parent()
        .unwrap()
        .join("http_cache");

    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create http_cache dir: {:?}", &dir))?;

    CACHE_DIR
        .set(dir)
        .map_err(|_| anyhow::anyhow!("http_cache already initialised"))?;

    log::info!("HTTP cache initialised at {:?}", CACHE_DIR.get().unwrap());
    Ok(())
}

fn cache_dir() -> Result<&'static PathBuf> {
    CACHE_DIR
        .get()
        .ok_or_else(|| anyhow::anyhow!("http_cache not initialised — call http_cache::init first"))
}

// ---------------------------------------------------------------------------
// Core: fetch with conditional-GET
// ---------------------------------------------------------------------------

/// Perform `GET <url>` using ETag revalidation.  Behaviour:
///
/// 1. If cache entry exists and `fetched_at + ttl > now` → return cached body
///    without making any HTTP request.
/// 2. Otherwise send GET with `If-None-Match` header when an ETag is known.
///    - 304 → touch `fetched_at`, return body from disk.  If the body file is
///      missing (e.g. evicted between meta write and now), re-fetch
///      unconditionally and treat it as a 200.
///    - 200 → update disk, return new body.
/// 3. On any network/timeout error, serve stale cache (if present) with a
///    `StaleFallback` source and a warning log.  If no stale entry exists,
///    propagate the error.
///
/// All disk I/O is serialized via a global async mutex so concurrent calls for
/// the same URL coalesce and eviction cannot race with a read.
///
/// The `client` is cloned from the caller (it is behind `Arc<Mutex>`).
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    ttl: Duration,
) -> Result<CachedBody> {
    let dir = cache_dir()?.to_path_buf();
    let key = hash_url(url);
    let meta_path = dir.join(format!("{}.meta.json", key));
    let body_path = dir.join(format!("{}.body", key));

    // Fast path: a fresh, intact cache entry can be served without the lock —
    // it is a read of two immutable-once-written files. If anything looks off
    // we fall through to the locked path and re-fetch.
    if meta_path.exists() && body_path.exists() {
        if let Ok(meta) = read_meta(&meta_path) {
            if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&meta.fetched_at) {
                let age = Utc::now() - fetched_at.with_timezone(&Utc);
                if age.to_std().unwrap_or(Duration::ZERO) < ttl {
                    if let Ok(bytes) = fs::read(&body_path) {
                        log::debug!("http_cache: fresh hit for {}", url);
                        return Ok(CachedBody {
                            bytes,
                            source: CacheSource::Fresh,
                        });
                    }
                }
            }
        }
    }

    // Serialize revalidation / writes across all callers.
    let _guard = fetch_lock().lock().await;

    // Re-check freshness under the lock: another call may have just populated
    // the cache while we were waiting.
    if meta_path.exists() && body_path.exists() {
        if let Ok(meta) = read_meta(&meta_path) {
            if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&meta.fetched_at) {
                let age = Utc::now() - fetched_at.with_timezone(&Utc);
                if age.to_std().unwrap_or(Duration::ZERO) < ttl {
                    if let Ok(bytes) = fs::read(&body_path) {
                        log::debug!("http_cache: fresh hit (locked) for {}", url);
                        return Ok(CachedBody {
                            bytes,
                            source: CacheSource::Fresh,
                        });
                    }
                }
            }
        }
    }

    // --- Network request with optional If-None-Match ---
    let etag = read_meta(&meta_path)
        .ok()
        .and_then(|m| m.etag.clone());

    let mut req = client.get(url);
    if let Some(ref et) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, et.as_str());
    }

    let resp = req.send().await;

    match resp {
        Ok(r) if r.status() == reqwest::StatusCode::NOT_MODIFIED => {
            // 304 — body on disk is still valid. If the body file is missing
            // (evicted/corrupted between meta write and now), a plain read
            // would fail; re-fetch unconditionally and handle as a 200.
            match fs::read(&body_path) {
                Ok(bytes) => {
                    log::debug!("http_cache: 304 revalidated for {}", url);
                    touch_meta_fetched_at(&meta_path, &etag, url);
                    Ok(CachedBody {
                        bytes,
                        source: CacheSource::Revalidated,
                    })
                }
                Err(e) => {
                    log::warn!(
                        "http_cache: 304 for {} but body missing ({}), re-fetching",
                        url, e
                    );
                    refetch_and_store(client, url, &meta_path, &body_path).await
                }
            }
        }
        Ok(r) if r.status().is_success() => store_response(r, url, &meta_path, &body_path, &dir).await,
        Ok(r) => {
            // Non-2xx, non-304 — try stale fallback
            let status = r.status();
            log::warn!("http_cache: unexpected status {} for {}", status, url);
            serve_stale_or_err(&body_path, status.as_u16(), url)
        }
        Err(e) => {
            // Network/timeout error — serve stale if we have it
            log::warn!("http_cache: network error for {}: {}", url, e);
            serve_stale_or_err(&body_path, 0, url)
        }
    }
}

/// Unconditional GET (no `If-None-Match`) used when a 304 arrives but the
/// cached body is missing. Persists the new body+meta and returns it as
/// `Updated`.
async fn refetch_and_store(
    client: &reqwest::Client,
    url: &str,
    meta_path: &PathBuf,
    body_path: &PathBuf,
) -> Result<CachedBody> {
    let r = client.get(url).send().await
        .context("http_cache: re-fetch failed after missing body")?;
    if !r.status().is_success() {
        let status = r.status();
        let body = r.text().await.unwrap_or_else(|_| "No body".to_string());
        bail!("http_cache: re-fetch for {} returned {}: {}", url, status, body);
    }
    store_response(r, url, meta_path, body_path, &cache_dir()?.to_path_buf()).await
}

/// Handle a successful (2xx) response: read body, atomically write body+meta,
/// enforce the size limit, and return `Updated`.
async fn store_response(
    r: reqwest::Response,
    url: &str,
    meta_path: &PathBuf,
    body_path: &PathBuf,
    dir: &PathBuf,
) -> Result<CachedBody> {
    let new_etag = r
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let bytes = r
        .bytes()
        .await
        .context("http_cache: failed to read response body")?
        .to_vec();

    // Write body via temp file then rename (atomic on same FS)
    let tmp_body = body_path.with_extension("body.tmp");
    fs::write(&tmp_body, &bytes)?;
    fs::rename(&tmp_body, body_path)?;

    write_meta(meta_path, &CacheMeta {
        etag: new_etag,
        fetched_at: Utc::now().to_rfc3339(),
        url: url.to_string(),
    })?;

    log::debug!("http_cache: 200 updated cache for {}", url);
    enforce_size_limit(dir)?;

    Ok(CachedBody {
        bytes,
        source: CacheSource::Updated,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_url(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hex::encode(hasher.finalize())
}

fn read_meta(path: &PathBuf) -> Result<CacheMeta> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

fn write_meta(path: &PathBuf, meta: &CacheMeta) -> Result<()> {
    let tmp = path.with_extension("meta.json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(meta)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Update only the `fetched_at` timestamp (for 304 revalidation).
fn touch_meta_fetched_at(path: &PathBuf, etag: &Option<String>, url: &str) {
    if let Ok(mut meta) = read_meta(path) {
        meta.fetched_at = Utc::now().to_rfc3339();
        // etag stays the same
        let _ = write_meta(path, &meta);
    } else {
        // No meta? write a fresh one
        let _ = write_meta(path, &CacheMeta {
            etag: etag.clone(),
            fetched_at: Utc::now().to_rfc3339(),
            url: url.to_string(),
        });
    }
}

fn serve_stale_or_err(body_path: &PathBuf, status_code: u16, url: &str) -> Result<CachedBody> {
    if body_path.exists() {
        let bytes = fs::read(body_path)
            .context("http_cache: failed to read stale body")?;
        log::warn!(
            "http_cache: serving stale cache for {} (status {})",
            url, status_code
        );
        Ok(CachedBody {
            bytes,
            source: CacheSource::StaleFallback,
        })
    } else {
        bail!(
            "http_cache: request to {} failed (status {}) and no cached body available",
            url, status_code
        );
    }
}

/// Delete oldest cache entries until total body size ≤ MAX_CACHE_SIZE_BYTES.
fn enforce_size_limit(dir: &PathBuf) -> Result<()> {
    let mut entries: Vec<(PathBuf, u64, String)> = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(".meta.json") {
            if let Ok(meta) = read_meta(&path) {
                let body_path = path.with_extension("").with_extension("body");
                let size = fs::metadata(&body_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                entries.push((body_path, size, meta.fetched_at));
            }
        }
    }

    let total: u64 = entries.iter().map(|(_, s, _)| *s).sum();
    if total <= MAX_CACHE_SIZE_BYTES {
        return Ok(());
    }

    // Sort by fetched_at ascending (oldest first)
    entries.sort_by(|a, b| a.2.cmp(&b.2));

    let mut remaining = total;
    for (body_path, size, _) in &entries {
        if remaining <= MAX_CACHE_SIZE_BYTES {
            break;
        }
        let meta_path = body_path
            .with_extension("")
            .with_extension("meta.json");
        let _ = fs::remove_file(body_path);
        let _ = fs::remove_file(meta_path);
        remaining -= size;
        log::debug!("http_cache: evicted {:?} ({} bytes)", body_path, size);
    }

    Ok(())
}

/// Encode bytes as lowercase hex string (simplified — no `hex` crate dep).
/// Read the cached body for a URL from disk without any network access.
pub fn read_body(url: &str) -> Option<Vec<u8>> {
    let dir = cache_dir().ok()?;
    std::fs::read(dir.join(format!("{}.body", hash_url(url)))).ok()
}

/// Read the stored ETag for a URL from disk.
pub fn read_etag(url: &str) -> Option<String> {
    let dir = cache_dir().ok()?;
    read_meta(&dir.join(format!("{}.meta.json", hash_url(url))))
        .ok()
        .and_then(|m| m.etag)
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}
