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
/// Hard ceiling on download attempts for ONE file per session. `MAX_DOWNLOAD_RETRIES`
/// counts only CONSECUTIVE fruitless attempts and resets whenever an attempt moved
/// the resume point forward — which is what an unstable connection needs, but on its
/// own it lets a server that hands over a byte and drops retry forever.
pub const MAX_DOWNLOAD_ATTEMPTS_PER_FILE: u32 = 60;
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
// Patch upload stage codes (`code` of `patch-upload-stage` / `patch-upload-finished`).
pub const ERR_PATCH_UPLOAD_ALREADY_RUNNING: &str = "PATCH_UPLOAD_ALREADY_RUNNING";
pub const ERR_RELEASE_EXISTS: &str = "RELEASE_EXISTS";
pub const ERR_UPDATES_REPO_NOT_FOUND: &str = "UPDATES_REPO_NOT_FOUND";
pub const WARN_INDEX_PUBLISH_FAILED: &str = "INDEX_PUBLISH_FAILED";
pub const WARN_SAVE_BREAKING: &str = "SAVE_BREAKING";
pub const WARN_PACK_SKIPPED_FILES: &str = "PACK_SKIPPED_FILES";
pub const WARN_TAG_PUSH_FAILED: &str = "TAG_PUSH_FAILED";
pub const SKIP_NO_GAME_SOURCE_DIR: &str = "NO_GAME_SOURCE_DIR";
// Progress events of the packer: the Pack view and patch uploads listen to
// different ones so a patch upload does not drive the Pack view.
pub const EVT_PACKING_PROGRESS: &str = "packing-progress";
pub const EVT_PATCH_PACK_PROGRESS: &str = "patch-pack-progress";
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
///
/// Applies to API calls only. In reqwest this is a TOTAL deadline that covers
/// the response body, so it must never reach a game-file download: a 2 GB
/// archive needs more than 17 MB/s to finish inside 120 s, and on any normal
/// connection every attempt died at exactly two minutes, forever.
pub const HTTP_REQUEST_TIMEOUT_SECS: u64 = 120;
/// Stall timeout: the longest gap allowed BETWEEN two reads. Unlike the total
/// deadline above it resets on every received chunk, so it bounds a dead
/// connection without putting a ceiling on how long a large file may take.
pub const HTTP_READ_TIMEOUT_SECS: u64 = 30;
/// Upper bound for a single game-file download, as a backstop against a server
/// that trickles bytes just fast enough to keep the read timeout happy.
/// Deliberately generous: a slow connection must be able to finish a multi-GB
/// archive in one attempt.
pub const DOWNLOAD_TOTAL_TIMEOUT_SECS: u64 = 6 * 60 * 60;

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

// ---------------------------------------------------------------------------
// Faction editor settings bundle (.gwfe)
// See plans/launcher/faction-editor-settings-bundle-plan.md for the spec.
// ---------------------------------------------------------------------------

/// Files/dirs the faction editor writes, relative to `gamedata/configs`.
pub const FE_CONFIG_LTX: &str = "faction_editor_config.ltx";
pub const FE_CONFIG_WRITE_LTX: &str = "faction_editor_config.write.ltx";
pub const FE_DEFAULT_CONFIG_LTX: &str = "faction_editor_default_config.ltx";
pub const FE_AXR_OPTIONS_LTX: &str = "axr_options.ltx";
pub const FE_AXR_OPTIONS_SECTION: &str = "mm_options";
pub const FE_GAME_RELATIONS_CUSTOM: &str = "creatures/game_relations_custom.ltx";
pub const FE_DEFAULT_CUSTOM_SIM: &str = "misc/simulations/default_custom.ltx";
pub const FE_SIM_OBJECTS_PROPS_CUSTOM: &str = "misc/simulation_objects_props_custom.ltx";
pub const FE_ARMAMENT_CUSTOM_DIR: &str = "misc/armament/custom";
pub const FE_SQUAD_DESCR_CUSTOM_DIR: &str = "misc/squad_descr/custom";
pub const FE_IS_CREATED_KEY: &str = "isCreated";

/// Bundle container.
pub const FE_BUNDLE_EXT: &str = "gwfe";
pub const FE_BUNDLE_KIND: &str = "gw-faction-editor-settings";
pub const FE_BUNDLE_SCHEMA: u32 = 1;
/// Entry inside the archive with the config, and the fragment carrying the
/// patched `axr_options.ltx` keys (see the bundle layout in the plan).
pub const FE_BUNDLE_CONFIG_PATH: &str = "configs/faction_editor_config.ltx";
pub const FE_BUNDLE_AXR_PARTIAL_PATH: &str = "configs/axr_options.partial.ltx";
pub const FE_BUNDLE_RELATIONS_PATH: &str = "configs/creatures/game_relations_custom.ltx";
pub const FE_BUNDLE_DEFAULT_CUSTOM_PATH: &str = "configs/misc/simulations/default_custom.ltx";
pub const FE_BUNDLE_SIM_PROPS_CUSTOM_PATH: &str = "configs/misc/simulation_objects_props_custom.ltx";
pub const FE_BUNDLE_ARMAMENT_CUSTOM_DIR: &str = "configs/misc/armament/custom";
pub const FE_BUNDLE_SQUAD_DESCR_CUSTOM_DIR: &str = "configs/misc/squad_descr/custom";

pub const FE_MAX_BUNDLE_SIZE: u64 = 20 * 1024 * 1024;
pub const FE_MAX_FILE_SIZE: u64 = 5 * 1024 * 1024;
pub const FE_MAX_NAME_LEN: usize = 64;
pub const FE_MAX_DESC_LEN: usize = 1024;
pub const FE_MAX_AUTHOR_LEN: usize = 64;
pub const FE_MAX_BACKUPS: usize = 5;
pub const FE_BACKUP_DIR: &str = "_backup";
pub const FE_PROFILES_DIR: &str = "faction-profiles";

/// axr_options.ltx keys the editor's "Common" tab writes; the editor's "reset
/// all" does not touch them, so `apply_defaults` leaves them alone too.
pub const FE_AXR_COMMON_FLAGS: &[&str] = &[
  "enable_events_without_player",
  "enable_events_with_azazel_mode",
  "enable_change_beh_factions",
  "enable_respawn_factions",
];

/// Props of `faction_editor_config.ltx` a patch is allowed to overwrite in the
/// player's config. Balance numbers only — visuals, names, descriptions, icons,
/// colors, sounds and the faction-existence flags stay the player's own
/// (see plans/launcher/faction-editor-patch-fields-plan.md §1.2).
///
/// `isCreated` / `isEnabled` are deliberately absent: `isCreated = false`
/// arriving in a patch would delete a faction the player created.
/// Every entry needs an `app.factionSettings.fields.<prop>` string in both
/// locales — `fe_patchable_fields_are_localized` is the sync check.
pub const FE_PATCHABLE_FIELDS: &[&str] = &[
  // base section [<faction>]
  "power",
  "power_leader",
  "leader_min_money",
  "leader_max_money",
  "leader_min_reputation",
  "leader_max_reputation",
  "fire_wound_immunity_leader",
  "explosion_immunity_leader",
  "fire_wound_preset",
  "explosion_preset",
  "mutant_alliance",
  // Presentation the mod authors rather than the player: `descr_diff` is the
  // 1..5 difficulty rating shown on the faction-select screen and has no
  // control in the editor at all, and the map marker color is regularly
  // retuned mod-side. A player who did recolor their faction can untick it in
  // the dialog; the rest of their visuals (names, icons, models, sounds) is
  // still never touched.
  "descr_diff",
  "spot_color_r",
  "spot_color_g",
  "spot_color_b",
  // rank sections [<faction>_<rank>]
  "min_money",
  "max_money",
  "min_reputation",
  "max_reputation",
  "fire_wound_immunity",
  "explosion_immunity",
];

/// Marker that a section holds model paths, not `key = value` pairs
/// (`[<faction>_visuals_<rank>]`). Never diffed, never patched.
pub const FE_VISUALS_SECTION_MARKER: &str = "_visuals_";

/// The mod's reference faction-editor config, spelled relative to the game
/// folder the developer picks when collecting a patch. Diffing THIS file
/// (never the developer's own `faction_editor_config.ltx`, which is excluded
/// from patches entirely) is what produces a patch's settings fragment.
pub const FE_DEFAULT_CONFIG_REL_PATH: &str = "gamedata/configs/faction_editor_default_config.ltx";

/// Path markers (globs, relative to the game root, `/`-separated) of files
/// whose change or deletion in a patch breaks existing save games: they sit
/// inside the game graph / AI maps, and rebuilding those renumbers game and
/// level vertices that old saves reference. Matched case-insensitively like
/// the exclude masks; a deleted marker file counts the same as a changed one.
pub const SAVE_BREAKING_GLOBS: &[&str] = &[
  "gamedata/spawns/all.spawn",
  "gamedata/levels/*/level.ai",
  "gamedata/levels/*/level.game",
  "gamedata/levels/*/level.spawn",
];

/// `appdata/<this>`: installed-patch markers and the settings fragments
/// patches carry. `patch_markers::patches_dir` builds the path; the pack
/// exclude in `handlers/patches.rs` spells it too, so keep them on one name.
pub const PATCHES_DIR_NAME: &str = "patches";

/// Subfolder of a patch folder holding the packed archives, their manifest
/// and `sha256.txt`. Kept after the upload so a patch can be re-uploaded by
/// hand; excluded from the archive it lives next to.
pub const PATCH_ARCHIVE_DIR: &str = "_archive";
/// Checksums of everything in `PATCH_ARCHIVE_DIR`, in the `sha256sum` format.
pub const PATCH_ARCHIVE_SHA_FILE: &str = "sha256.txt";

/// Name of the fragment a patch carries, inside the patch archive and then in
/// the player's `appdata/patches`: `<patch tag>` + this suffix.
pub const FE_PATCH_FRAGMENT_SUFFIX: &str = ".faction_editor_patch.ltx";
/// Staging name used while the patch is being collected — at that point the
/// patch tag is not known yet; `upload_patch` renames it.
pub const FE_PATCH_FRAGMENT_STAGING: &str = "_pending.faction_editor_patch.ltx";
/// Hard cap on a fragment arriving from the network.
pub const FE_MAX_PATCH_FRAGMENT_SIZE: u64 = 256 * 1024;

// Faction-editor bundle error codes (returned as Err strings to the frontend).
pub const FE_ERR_NO_VERSION: &str = "FE_ERR_NO_VERSION";
pub const FE_ERR_NO_CONFIG: &str = "FE_ERR_NO_CONFIG";
pub const FE_ERR_GAME_RUNNING: &str = "FE_ERR_GAME_RUNNING";
pub const FE_ERR_EXPORT_NOT_GWFE: &str = "FE_ERR_EXPORT_NOT_GWFE";
pub const FE_ERR_BUNDLE_NOT_ZIP: &str = "FE_ERR_BUNDLE_NOT_ZIP";
pub const FE_ERR_BUNDLE_NO_MANIFEST: &str = "FE_ERR_BUNDLE_NO_MANIFEST";
pub const FE_ERR_BUNDLE_KIND: &str = "FE_ERR_BUNDLE_KIND";
pub const FE_ERR_BUNDLE_SCHEMA: &str = "FE_ERR_BUNDLE_SCHEMA";
pub const FE_ERR_BUNDLE_UNKNOWN_FILE: &str = "FE_ERR_BUNDLE_UNKNOWN_FILE";
pub const FE_ERR_BUNDLE_TOO_LARGE: &str = "FE_ERR_BUNDLE_TOO_LARGE";
pub const FE_ERR_BUNDLE_HASH_MISMATCH: &str = "FE_ERR_BUNDLE_HASH_MISMATCH";
pub const FE_ERR_BUNDLE_INVALID_CONFIG: &str = "FE_ERR_BUNDLE_INVALID_CONFIG";
pub const FE_ERR_BUNDLE_INVALID_AXR_KEYS: &str = "FE_ERR_BUNDLE_INVALID_AXR_KEYS";
pub const FE_ERR_APPLY_FAILED: &str = "FE_ERR_APPLY_FAILED";
pub const FE_ERR_ROLLBACK_FAILED: &str = "FE_ERR_ROLLBACK_FAILED";
pub const FE_ERR_PROFILE_EXISTS: &str = "FE_ERR_PROFILE_EXISTS";
pub const FE_ERR_PROFILE_NAME_INVALID: &str = "FE_ERR_PROFILE_NAME_INVALID";
pub const FE_ERR_PROFILE_NOT_FOUND: &str = "FE_ERR_PROFILE_NOT_FOUND";
pub const FE_ERR_PATCH_INVALID: &str = "FE_ERR_PATCH_INVALID";
pub const FE_ERR_PATCH_EMPTY: &str = "FE_ERR_PATCH_EMPTY";
pub const FE_ERR_PATCH_NOT_FOUND: &str = "FE_ERR_PATCH_NOT_FOUND";

// Non-fatal warnings surfaced to the frontend alongside a successful result.
pub const FE_WARN_AXR_OPTIONS_MISSING: &str = "FE_WARN_AXR_OPTIONS_MISSING";
pub const FE_WARN_AXR_OPTIONS_SECTION_MISSING: &str = "FE_WARN_AXR_OPTIONS_SECTION_MISSING";
pub const FE_WARN_UNKNOWN_FACTIONS: &str = "FE_WARN_UNKNOWN_FACTIONS";
pub const FE_WARN_PATCH_NO_CONFIG: &str = "FE_WARN_PATCH_NO_CONFIG";
pub const FE_WARN_PATCH_NO_WRITE_CONFIG: &str = "FE_WARN_PATCH_NO_WRITE_CONFIG";
pub const FE_WARN_PATCH_SKIPPED_SECTIONS: &str = "FE_WARN_PATCH_SKIPPED_SECTIONS";

#[cfg(test)]
mod fe_patch_tests {
  use super::*;

  /// Every prop a patch may carry, and every `FE_*_PATCH_*` code the backend
  /// can return, needs a string in BOTH locales — otherwise the player sees a
  /// raw key like `fire_wound_immunity_leader` in the apply dialog. There is
  /// no codegen between Rust and the locale files, so this is the sync check
  /// (same role as `frontend_alife_consts_stay_in_sync` above).
  #[test]
  fn fe_patchable_fields_and_codes_are_localized() {
    for locale in ["ru", "en"] {
      let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../src/locales/{}.json", locale));
      let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must be readable: {}", path.display(), e));
      let json: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} must be valid JSON: {}", path.display(), e));

      let mut check = |pointer: &str, key: &str| {
        let full = format!("{}/{}", pointer, key);
        let value = json.pointer(&full).and_then(|v| v.as_str()).unwrap_or_default();
        assert!(!value.trim().is_empty(), "{}: missing or empty '{}'", locale, full.replace('/', "."));
      };

      for prop in FE_PATCHABLE_FIELDS {
        check("/app/factionSettings/fields", prop);
      }
      for code in [FE_ERR_PATCH_INVALID, FE_ERR_PATCH_EMPTY, FE_ERR_PATCH_NOT_FOUND] {
        check("/app/factionSettings/errors", code);
      }
      for code in [FE_WARN_PATCH_NO_CONFIG, FE_WARN_PATCH_NO_WRITE_CONFIG, FE_WARN_PATCH_SKIPPED_SECTIONS] {
        check("/app/factionSettings/warnings", code);
      }
    }
  }

  /// A prop listed twice would make the "collect patch" screen show duplicate
  /// checkboxes and the dialog a duplicate line.
  #[test]
  fn fe_patchable_fields_have_no_duplicates() {
    let mut seen: Vec<&str> = FE_PATCHABLE_FIELDS.to_vec();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(before, seen.len(), "FE_PATCHABLE_FIELDS contains duplicates");
  }
}
