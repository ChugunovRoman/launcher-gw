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
/// Search API (/search/issues) — user data and manifest lookup.
pub const CACHE_TTL_SEARCH_API_SECS: u64 = 86400; // 24 hours
/// Raw files (manifest.json and similar).
pub const CACHE_TTL_RAW_FILE_SECS: u64 = 600; // 10 min
/// Launcher background image.
pub const CACHE_TTL_BACKGROUND_SECS: u64 = 86400; // 24 hours

// User data cache TTLs (persisted in config.json, separate from http_cache).
/// Positive cache hit (user found with flags).
pub const USER_CACHE_TTL_POSITIVE_SECS: u64 = 86400; // 24 hours
/// Negative cache hit (no issue found).
pub const USER_CACHE_TTL_NEGATIVE_SECS: u64 = 21600; // 6 hours

/// Default git branch used when uploading the manifest and creating a tag.
/// TODO: this is a temporary crutch. The correct fix is to fetch the repo's
/// default branch from the provider and thread it through `add_file_to_repo` /
/// `create_tag` / `create_release` (which currently hardcode "master" on the
/// provider side too — see Github::__create_release `target_commitish`).
pub const DEFAULT_BRANCH: &str = "master";
