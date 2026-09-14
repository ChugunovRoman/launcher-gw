pub const REPO_LAUNCGER_ID: u32 = 75545910;
pub const REPO_LAUNCGER_ID_2: u32 = 77354883;
pub const GITHUB_HOST: &str = "https://github.com";
pub const GITHUB_API_HOST: &str = "https://api.github.com";
pub const MAIN_DEVELOPER_NAME: &str = "ChugunovRoman";
pub const GITHUB_ORG: &str = "Global-War-Releases";
pub const GITHUB_LAUNCHER_REPO_NAME: &str = "launcher-gw";

pub const GITLAB_API_HOST: &str = "https://gitlab.com/api/v4";

pub const MANIFEST_NAME: &str = "manifest.json";
pub const VERSIONS_DIR: &str = "versions";

pub const EXE_WIN_NAME: &str = "Launcher.exe";
pub const EXE_LINUX_NAME: &str = "Launcher";
pub const BASE_DIR: &str = "com.ruut.stalker";
pub const CONFIG_NAME: &str = "config.json";

pub const BIN_DIR: &str = "bin";
pub const APPDATA_DIR: &str = "appdata";
pub const GAMEDATA_DIR: &str = "gamedata";
pub const CONFIGS_DIR: &str = "configs";
pub const ALIFE_LTX: &str = "alife.ltx";
pub const ALIFE_SECTION: &str = "alife";

// [alife] keys managed by the launcher. Defaults mirror gamedata/configs/alife.ltx
// so old config.json files keep the game's current values until an explicit edit.
pub const ALIFE_KEY_OBJECTS_PER_UPDATE: &str = "objects_per_update";
pub const ALIFE_KEY_POSITION_UPDATE_INTERVAL_MS: &str = "position_update_interval_ms";
pub const ALIFE_KEY_PROCESS_TIME: &str = "process_time";
pub const ALIFE_KEY_SWITCH_DISTANCE: &str = "switch_distance";

pub const ALIFE_DEFAULT_OBJECTS_PER_UPDATE: u32 = 20;
pub const ALIFE_DEFAULT_POSITION_UPDATE_INTERVAL_MS: u32 = 5000;
pub const ALIFE_DEFAULT_PROCESS_TIME: i32 = 500;
pub const ALIFE_DEFAULT_SWITCH_DISTANCE: f32 = 250.0;

// Value ranges for the alife overrides (same bounds as the settings UI TrackBars).
// switch_distance follows the in-game menu range; the published presets go up
// to 600 (quality), so the max must not be lower than that.
pub const ALIFE_MIN_OBJECTS_PER_UPDATE: u32 = 1;
pub const ALIFE_MAX_OBJECTS_PER_UPDATE: u32 = 100;
pub const ALIFE_MIN_POSITION_UPDATE_INTERVAL_MS: u32 = 0;
pub const ALIFE_MAX_POSITION_UPDATE_INTERVAL_MS: u32 = 10000;
pub const ALIFE_MIN_PROCESS_TIME: i32 = 100;
pub const ALIFE_MAX_PROCESS_TIME: i32 = 5000;
pub const ALIFE_MIN_SWITCH_DISTANCE: f32 = 30.0;
pub const ALIFE_MAX_SWITCH_DISTANCE: f32 = 1000.0;

#[cfg(test)]
mod tests {
  use super::*;

  /// src/consts.ts duplicates the alife numbers for the UI (ALIFE_DEFAULTS /
  /// ALIFE_RANGES); there is no codegen between the two files, so this test
  /// is the sync check. When it fails, edit both sides together.
  #[test]
  fn frontend_alife_consts_stay_in_sync() {
    let ts_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/consts.ts");
    let ts = std::fs::read_to_string(&ts_path).expect("src/consts.ts must be readable");

    assert!(
      ts.contains("objects_per_update: 20,"),
      "ALIFE_DEFAULTS.objects_per_update is out of sync with ALIFE_DEFAULT_OBJECTS_PER_UPDATE"
    );
    assert!(
      ts.contains("position_update_interval_ms: 5000,"),
      "ALIFE_DEFAULTS.position_update_interval_ms is out of sync with ALIFE_DEFAULT_POSITION_UPDATE_INTERVAL_MS"
    );
    assert!(
      ts.contains("process_time: 500,"),
      "ALIFE_DEFAULTS.process_time is out of sync with ALIFE_DEFAULT_PROCESS_TIME"
    );
    assert!(
      ts.contains("switch_distance: 250,"),
      "ALIFE_DEFAULTS.switch_distance is out of sync with ALIFE_DEFAULT_SWITCH_DISTANCE"
    );

    assert!(
      ts.contains("objects_per_update: { min: 1, max: 100, step: 1 }"),
      "ALIFE_RANGES.objects_per_update is out of sync with ALIFE_MIN_/MAX_OBJECTS_PER_UPDATE"
    );
    assert!(
      ts.contains("position_update_interval_ms: { min: 0, max: 10000, step: 100 }"),
      "ALIFE_RANGES.position_update_interval_ms is out of sync with ALIFE_MIN_/MAX_POSITION_UPDATE_INTERVAL_MS"
    );
    assert!(
      ts.contains("process_time: { min: 100, max: 5000, step: 50 }"),
      "ALIFE_RANGES.process_time is out of sync with ALIFE_MIN_/MAX_PROCESS_TIME"
    );
    assert!(
      ts.contains("switch_distance: { min: 30, max: 1000, step: 5 }"),
      "ALIFE_RANGES.switch_distance is out of sync with ALIFE_MIN_/MAX_SWITCH_DISTANCE"
    );
  }
}
pub const SCRIPTS_DIR: &str = "scripts";
pub const SCRIPT_G: &str = "_g.script";
pub const USER_LTX: &str = "user.ltx";
pub const TMP_LTX: &str = "tmp.ltx";
pub const FSGAME_LTX: &str = "fsgame.ltx";

pub const NO_KEY: &str = "---";

/// user.ltx commands whose SECOND token is a name, not a value
/// (`bind <action> <key>`, `bind_sec <action> <key>`). Every other command is a
/// flat `cvar value` pair whose value may itself contain spaces
/// (`vid_mode 1920x1080`, `player_name John Doe`). Nesting used to be guessed
/// from the number of spaces on the line, which misparsed every bind line with
/// a trailing comment or a double space.
pub const LTX_NESTED_COMMANDS: &[&str] = &["bind", "bind_sec"];
pub const DEFAULT_BIND_LTX: &str = "default.ltx";
pub const CUSTOM_BIND_LTX: &str = "custom.ltx";

// Providers ids
pub const GITLAB_PID: &str = "gitlab";
pub const GITHUB_PID: &str = "github";

pub const PULL_FILES_SIZE: u8 = 1;

// Download/upload integrity retry limits (per file, not per worker).
/// Network errors: connection resets, HTTP failures, interrupted streams.
pub const MAX_DOWNLOAD_RETRIES: u32 = 5;
/// Size/hash mismatches of a completed download.
pub const MAX_VERIFY_RETRIES: u32 = 3;
/// Asset hash mismatches detected on the server after an upload.
pub const MAX_UPLOAD_VERIFY_RETRIES: u32 = 3;

// Command-level error codes (returned as Err strings to the frontend).
pub const ERR_USER_CANCELLED: &str = "USER_CANCELLED";
pub const ERR_DOWNLOAD_FAILED: &str = "DOWNLOAD_FAILED";
pub const ERR_UPLOAD_HASH_MISMATCH: &str = "UPLOAD_HASH_MISMATCH";
pub const ERR_RELEASE_NOT_IN_INDEX: &str = "RELEASE_NOT_IN_INDEX";
pub const ERR_DOWNLOAD_ALREADY_RUNNING: &str = "DOWNLOAD_ALREADY_RUNNING";
pub const ERR_VERIFY_ALREADY_RUNNING: &str = "VERIFY_ALREADY_RUNNING";
/// Manifest reconciliation refused: the server release carries no assets at
/// all (index published before the upload finished, or an empty API answer).
pub const ERR_RELEASE_NO_ASSETS: &str = "RELEASE_NO_ASSETS";
/// Manifest reconciliation refused: the server release lists drastically fewer
/// assets than the saved progress, so "everything was deleted" is not believed.
pub const ERR_RELEASE_ASSETS_SHRUNK: &str = "RELEASE_ASSETS_SHRUNK";
/// Token cannot be used as an `Authorization` header value (newline / non-ASCII).
pub const ERR_INVALID_TOKEN: &str = "INVALID_TOKEN";
/// Keybind profile export destination has no `.ltx` extension.
pub const ERR_EXPORT_NOT_LTX: &str = "EXPORT_NOT_LTX";

// Per-file error codes carried by the `download-version-file-error` event.
pub const FILE_ERR_HASH_MISMATCH: &str = "HASH_MISMATCH";
pub const FILE_ERR_SIZE_MISMATCH: &str = "SIZE_MISMATCH";
pub const FILE_ERR_NETWORK: &str = "NETWORK";
pub const FILE_ERR_UNPACK_FAILED: &str = "UNPACK_FAILED";
pub const FILE_ERR_COPY_FAILED: &str = "COPY_FAILED";
pub const FILE_ERR_VERIFY_FAILED: &str = "VERIFY_FAILED";
pub const FILE_ERR_BAD_MANIFEST: &str = "BAD_MANIFEST";

/// Marker prefix for "the remote file changed between read and write".
/// The frontend matches on it to tell the maintainer to rebuild the index
/// preview instead of retrying blindly (R12).
pub const INDEX_CONFLICT_ERR: &str = "INDEX_CONFLICT";

// Static release index (player-side, raw CDN — not counted against API rate limit)
// Per-provider: each provider gets its own index with provider-specific URLs.
// The writer publishes the index for the *currently selected* provider; the
// reader reads the index of the *currently active* provider (which may be a
// fallback if the saved provider is down).

/// GitHub index: repo `Global-War-Releases/index`, branch `master`.
pub const GITHUB_INDEX_RAW_URL: &str =
  "https://raw.githubusercontent.com/Global-War-Releases/index/master/index.json";
/// GitLab index: project `index` in the root group.
/// `0` = not configured (reader falls back to API, writer skips silently).
/// Fill with the real numeric project id after creating the GitLab index project.
pub const GITLAB_INDEX_PROJECT_ID: u32 = 85506224;
pub const INDEX_REPO_NAME: &str = "index";
pub const INDEX_SCHEMA_VERSION: u32 = 1;
pub const INDEX_CACHE_TTL_SECS: u64 = 600; // 10 min

// HTTP cache TTLs (seconds).  Tune these to balance freshness vs API usage.
/// Org repos listing (paginated GET /orgs/{org}/repos).
pub const CACHE_TTL_ORG_REPOS_SECS: u64 = 3600; // 1 hour
/// Single release metadata (releases/latest, /repos/.../releases).
pub const CACHE_TTL_RELEASE_SECS: u64 = 600; // 10 min
/// Search API (/search/issues) — legacy manifest.json issue lookup.
/// (User data no longer goes through Search — see `service::client::get_user`,
/// which reads the `users` map in the static release index instead.)
pub const CACHE_TTL_SEARCH_API_SECS: u64 = 86400; // 24 hours
/// Raw files (manifest.json and similar).
pub const CACHE_TTL_RAW_FILE_SECS: u64 = 600; // 10 min
/// Launcher background image.
pub const CACHE_TTL_BACKGROUND_SECS: u64 = 86400; // 24 hours

// HTTP client timeouts (seconds).  Mirrors the values used by the Github /
// Gitlab clients: without them a silently dropping network (captive portal,
// firewall) leaves a request pending forever while a global lock is held.
/// TCP/TLS connect timeout.
pub const HTTP_CONNECT_TIMEOUT_SECS: u64 = 15;
/// Whole-request timeout (connect + headers + body).
pub const HTTP_REQUEST_TIMEOUT_SECS: u64 = 120;

/// How many bytes of a response body are quoted in a "failed to parse JSON"
/// error. Enough to recognise an HTML captive-portal page or an API error
/// object without dumping a whole response into the log.
pub const JSON_ERROR_BODY_PREVIEW_LEN: usize = 300;

/// Default git branch used when uploading the manifest and creating a tag.
/// TODO: this is a temporary crutch. The correct fix is to fetch the repo's
/// default branch from the provider and thread it through `add_file_to_repo` /
/// `create_tag` / `create_release` (which currently hardcode "master" on the
/// provider side too — see Github::__create_release `target_commitish`).
pub const DEFAULT_BRANCH: &str = "master";

/// GitLab generic package namespace used for release assets
/// (`packages/generic/<namespace>/<tag>/<file>`). Kept in consts so the
/// package-lookup API calls match the upload URLs.
pub const GENERIC_PACKAGE_NAMESPACE: &str = "gw_releases";

/// Error prefix used when the downloaded launcher binary does not match the
/// SHA-256 published in the release index. The file replaces the RUNNING
/// executable, so a mismatch is always terminal — the download is deleted.
pub const LAUNCHER_SHA256_MISMATCH: &str = "Launcher download sha256 mismatch";

// Pack (release building) error prefixes. The Pack view shows the message as
// is; the prefix makes the cause greppable in the launcher log.
/// The chosen source folder does not exist (validated before the target dir is
/// cleaned — item 80).
pub const PACK_ERR_SOURCE_NOT_FOUND: &str = "PACK_SOURCE_NOT_FOUND";
/// Chunk size must be a positive number of megabytes.
pub const PACK_ERR_INVALID_CHUNK_SIZE: &str = "PACK_INVALID_CHUNK_SIZE: chunk size must be greater than 0 MB";
/// The source folder contains no files to pack (after applying the exclude masks).
pub const PACK_ERR_NO_SOURCE_FILES: &str = "PACK_NO_SOURCE_FILES: no files found in the source directory — nothing to pack";
/// A directory walk error during packing: an incomplete release is worse than a
/// refused one, so the pack aborts (items 75 and 84).
pub const PACK_ERR_WALK_FAILED: &str = "PACK_WALK_FAILED";
/// Every source file turned out to be unreadable — nothing was stored.
pub const PACK_ERR_NOTHING_PACKED: &str = "PACK_NOTHING_PACKED";

/// How many already-extracted file names are quoted in the log when unpacking
/// fails. The archive can hold tens of thousands of entries, so the list is
/// capped: it is a starting point for a manual cleanup, not a full inventory
/// (the destination directory itself is logged next to it).
pub const UNPACK_EXTRACTED_LOG_LIMIT: usize = 100;

/// How many skipped (unsafe-named) archive entries are quoted in the log.
/// Skipping is non-fatal, so this is a diagnostic hint only.
pub const UNPACK_SKIPPED_LOG_LIMIT: usize = 50;
