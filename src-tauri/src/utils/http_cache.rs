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
///
/// Timeouts are mandatory here: `fetch` holds the global `FETCH_LOCK` across
/// `send().await`, so a request that never completes (captive portal, silently
/// dropping firewall) would block every other cache user forever.
/// `Client::new()` has no timeouts at all — the values mirror the Github /
/// Gitlab clients.
pub static SHARED_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(crate::consts::HTTP_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(crate::consts::HTTP_REQUEST_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|e| {
            log::error!("http_cache: failed to build SHARED_CLIENT with timeouts ({}), falling back to the default client", e);
            reqwest::Client::new()
        })
});

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
    fetch_inner(client, url, Some(ttl)).await
}

/// Like [`fetch`], but ALWAYS goes to the network (no TTL fast path).
///
/// Used by explicit "Refresh" actions: with a plain TTL the org-repos listing
/// was served from disk for a whole hour, so the button could not pick up a
/// newly published release. The ETag is still sent, so the usual answer is
/// `304 Not Modified` — the body comes from disk and almost no traffic is
/// spent. On a network error the stale cache is served exactly as in `fetch`.
pub async fn fetch_force(client: &reqwest::Client, url: &str) -> Result<CachedBody> {
    fetch_inner(client, url, None).await
}

/// Shared implementation. `ttl == None` means "never serve from the TTL fast
/// path — always revalidate".
async fn fetch_inner(
    client: &reqwest::Client,
    url: &str,
    ttl: Option<Duration>,
) -> Result<CachedBody> {
    let dir = cache_dir()?.to_path_buf();
    let key = hash_url(url);
    let meta_path = dir.join(format!("{}.meta.json", key));
    let body_path = dir.join(format!("{}.body", key));

    // Fast path: a fresh, intact cache entry can be served without the lock —
    // it is a read of two immutable-once-written files. If anything looks off
    // we fall through to the locked path and re-fetch.
    if let (Some(ttl), true) = (ttl, meta_path.exists() && body_path.exists()) {
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
    if let (Some(ttl), true) = (ttl, meta_path.exists() && body_path.exists()) {
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
            serve_stale_or_err(&body_path, &meta_path, status.as_u16(), url)
        }
        Err(e) => {
            // Network/timeout error — serve stale if we have it
            log::warn!("http_cache: network error for {}: {}", url, e);
            serve_stale_or_err(&body_path, &meta_path, 0, url)
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

/// Maximum age of a stale entry that may still be served when the live request
/// failed (plan decision Q3: 7 days).  A body older than this is no longer
/// trustworthy, so the caller gets an error and falls back to its own source
/// (e.g. the live API) instead of silently presenting week-old data as current.
///
/// Applies to BOTH failure kinds — a server error and no network at all.  The
/// consequence for a player who has been offline for over a week is an explicit
/// error instead of a stale release list; that is the agreed behaviour (B4 was
/// precisely "stale of unlimited age is served as valid").
const MAX_STALE_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Serve the cached body when the live request failed (`status_code == 0`
/// means a network/timeout error, anything else is the server's status).
/// The entry must be younger than `MAX_STALE_AGE`; a missing or unreadable
/// `.meta.json` counts as "age unknown" and is refused for the same reason.
fn serve_stale_or_err(body_path: &PathBuf, meta_path: &PathBuf, status_code: u16, url: &str) -> Result<CachedBody> {
    if !body_path.exists() {
        bail!(
            "http_cache: request to {} failed (status {}) and no cached body available",
            url, status_code
        );
    }

    let age = read_meta(meta_path).ok().and_then(|meta| {
        chrono::DateTime::parse_from_rfc3339(&meta.fetched_at)
            .ok()
            .map(|fetched| Utc::now().signed_duration_since(fetched.with_timezone(&Utc)))
    });
    match age {
        // A negative age means the timestamp is in the future (clock skew, DST
        // rollback) — clamp to 0 and treat the entry as fresh instead of
        // reporting a nonsensical "too old (-0 days)".
        Some(age) if (age.num_seconds().max(0) as u64) < MAX_STALE_AGE.as_secs() => {}
        Some(age) => bail!(
            "http_cache: request to {} failed (status {}) and the cached body is too old ({} days)",
            url,
            status_code,
            age.num_days()
        ),
        None => bail!(
            "http_cache: request to {} failed (status {}) and the cached body has no usable timestamp",
            url, status_code
        ),
    }

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
}

/// Delete oldest cache entries until total body size ≤ MAX_CACHE_SIZE_BYTES.
/// Also removes orphaned `.body` files (no matching `.meta.json`) and stale
/// `.tmp` files that are older than a few minutes.
fn enforce_size_limit(dir: &PathBuf) -> Result<()> {
    let mut entries: Vec<(PathBuf, u64, String)> = Vec::new();
    let mut known_bodies: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

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
                known_bodies.insert(body_path.clone());
                entries.push((body_path, size, meta.fetched_at));
            }
        } else if name.ends_with(".tmp") {
            // Stale temp files from interrupted writes — remove if older than 5 min.
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    if modified.elapsed().unwrap_or_default().as_secs() > 300 {
                        let _ = fs::remove_file(&path);
                        log::debug!("http_cache: removed stale tmp {:?}", path);
                    }
                }
            }
        }
    }

    // Remove orphaned .body files that have no matching .meta.json
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(".body") && !known_bodies.contains(&path) {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let _ = fs::remove_file(&path);
            log::debug!("http_cache: removed orphaned body {:?} ({} bytes)", path, size);
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

/// Overwrite the on-disk cache entry for `url` with `bytes`, as if it had
/// just been fetched over the network (`fetched_at = now`, no ETag).
///
/// Used right after writing `bytes` to the remote resource itself (e.g.
/// committing a new `index.json`) so a `fetch` call made moments later sees
/// the fresh content immediately — without waiting out the TTL and without
/// depending on the origin's own CDN having already propagated the commit
/// (raw.githubusercontent.com in particular can lag a push by a few seconds).
/// No ETag is stored, so the next real revalidation after TTL expiry falls
/// back to a plain GET instead of a conditional one — a one-time cost.
/// Write a cache entry under the global fetch lock so body and metadata
/// cannot be interleaved with a concurrent `fetch` or `clear_all` (R9 fix).
pub async fn store(url: &str, bytes: &[u8]) -> Result<()> {
    let _guard = fetch_lock().lock().await;
    let dir = cache_dir()?;
    let key = hash_url(url);
    let meta_path = dir.join(format!("{}.meta.json", key));
    let body_path = dir.join(format!("{}.body", key));

    let tmp_body = body_path.with_extension("body.tmp");
    fs::write(&tmp_body, bytes)?;
    fs::rename(&tmp_body, &body_path)?;

    write_meta(&meta_path, &CacheMeta {
        etag: None,
        fetched_at: Utc::now().to_rfc3339(),
        url: url.to_string(),
    })?;

    enforce_size_limit(dir)?;

    Ok(())
}

/// Read the stored ETag for a URL from disk.
pub fn read_etag(url: &str) -> Option<String> {
    let dir = cache_dir().ok()?;
    read_meta(&dir.join(format!("{}.meta.json", hash_url(url))))
        .ok()
        .and_then(|m| m.etag)
}

/// Remove all cached files from the disk cache directory.
/// Called when the API token changes so that private responses cached under
/// the anonymous context are not served after authentication (or vice versa).
/// Remove all cached files under the global fetch lock so a concurrent
/// `fetch` or `store` cannot leave orphaned body/metadata pairs (R9 fix).
pub async fn clear_all() {
    let _guard = fetch_lock().lock().await;
    let dir = match cache_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    log::info!("http_cache: cleared all cached entries");
}

/// Drop every cached response whose URL contains `needle`.
///
/// Needed because the TTL is what makes a listing stale, not an explicit
/// invalidation: right after creating a release the launcher rebuilds the
/// index, and a releases listing cached minutes earlier would not contain the
/// release that was just created, so the published index would silently omit
/// it. Returns how many entries were dropped.
pub async fn invalidate_urls_containing(needle: &str) -> usize {
    let _guard = fetch_lock().lock().await;
    let dir = match cache_dir() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let dropped = invalidate_in_dir(dir, needle);
    if dropped > 0 {
        log::info!("http_cache: dropped {} entry(ies) matching '{}'", dropped, needle);
    }
    dropped
}

/// Body of `invalidate_urls_containing`, split out so it can be tested against
/// a temp directory (the real cache dir is a process-wide `OnceLock`).
fn invalidate_in_dir(dir: &std::path::Path, needle: &str) -> usize {
    let mut dropped = 0usize;
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(hash) = name.strip_suffix(".meta.json") else { continue };
        // A meta file that cannot be parsed has no URL to match on; leaving it
        // alone keeps this from turning into a blanket cache wipe.
        let Ok(meta) = read_meta(&path) else { continue };
        if !meta.url.contains(needle) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(dir.join(format!("{}.body", hash)));
        dropped += 1;
    }

    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidate_in_dir_drops_only_matching_pairs() {
        let dir = std::env::temp_dir().join(format!("http_cache_inv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let write = |hash: &str, url: &str| {
            let meta = CacheMeta { etag: None, fetched_at: chrono::Utc::now().to_rfc3339(), url: url.to_owned() };
            fs::write(dir.join(format!("{}.meta.json", hash)), serde_json::to_vec(&meta).unwrap()).unwrap();
            fs::write(dir.join(format!("{}.body", hash)), b"x").unwrap();
        };
        write("aaa", "https://api.github.com/repos/o/r/releases?per_page=100&page=1");
        write("bbb", "https://raw.githubusercontent.com/o/index/master/index.json");
        write("ccc", "https://gitlab.com/api/v4/projects/1/releases");
        // A corrupt meta file must be left alone rather than blindly removed.
        fs::write(dir.join("ddd.meta.json"), b"not json").unwrap();
        fs::write(dir.join("ddd.body"), b"x").unwrap();

        assert_eq!(invalidate_in_dir(&dir, "/releases"), 2);

        for gone in ["aaa.meta.json", "aaa.body", "ccc.meta.json", "ccc.body"] {
            assert!(!dir.join(gone).exists(), "{} must be dropped", gone);
        }
        for kept in ["bbb.meta.json", "bbb.body", "ddd.meta.json", "ddd.body"] {
            assert!(dir.join(kept).exists(), "{} must be kept", kept);
        }

        let _ = fs::remove_dir_all(&dir);
    }
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
