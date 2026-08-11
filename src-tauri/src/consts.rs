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

/// Default git branch used when uploading the manifest and creating a tag.
/// TODO: this is a temporary crutch. The correct fix is to fetch the repo's
/// default branch from the provider and thread it through `add_file_to_repo` /
/// `create_tag` / `create_release` (which currently hardcode "master" on the
/// provider side too — see Github::__create_release `target_commitish`).
pub const DEFAULT_BRANCH: &str = "master";
