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
pub const DEFAULT_BIND_LTX: &str = "default.ltx";
pub const CUSTOM_BIND_LTX: &str = "custom.ltx";

// Providers ids
pub const GITLAB_PID: &str = "gitlab";
pub const GITHUB_PID: &str = "github";

pub const PULL_FILES_SIZE: u8 = 1;

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

/// Default git branch used when uploading the manifest and creating a tag.
/// TODO: this is a temporary crutch. The correct fix is to fetch the repo's
/// default branch from the provider and thread it through `add_file_to_repo` /
/// `create_tag` / `create_release` (which currently hardcode "master" on the
/// provider side too — see Github::__create_release `target_commitish`).
pub const DEFAULT_BRANCH: &str = "master";
