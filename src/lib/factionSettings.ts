import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import { factionProfiles, factionVersionName } from "../store/factionSettings";

/** The game version this screen reads from / applies to. Sent with every
 * version-dependent command so the screen is not tied to the version selected
 * for launching. */
function versionName(): string | undefined {
  return get(factionVersionName) || undefined;
}

export async function loadFactionProfiles(): Promise<FactionProfileItem[]> {
  const list = await invoke<FactionProfileItem[]>("fe_profiles_list");
  factionProfiles.set(list);
  return list;
}

export function isFactionGameRunning(): Promise<boolean> {
  return invoke<boolean>("fe_is_game_running", { versionName: versionName() });
}

export function inspectFactionBundle(path: string): Promise<FactionBundleInspectResult> {
  return invoke<FactionBundleInspectResult>("fe_inspect_bundle", { path, versionName: versionName() });
}

export function importFactionBundle(path: string): Promise<FactionApplyResult> {
  return invoke<FactionApplyResult>("fe_import_bundle", { path, versionName: versionName() });
}

export function exportFactionBundle(destPath: string, name: string, description: string, author: string): Promise<void> {
  return invoke<void>("fe_export_bundle", { destPath, name, description, author, versionName: versionName() });
}

export function resetFactionDefaults(): Promise<FactionApplyResult> {
  return invoke<FactionApplyResult>("fe_reset_to_default", { versionName: versionName() });
}

export function saveCurrentFactionProfile(name: string, description: string, author: string): Promise<FactionProfileItem> {
  return invoke<FactionProfileItem>("fe_profile_save_current", { name, description, author, versionName: versionName() });
}

export function inspectFactionProfile(id: string): Promise<FactionBundleInspectResult> {
  return invoke<FactionBundleInspectResult>("fe_profile_inspect", { id, versionName: versionName() });
}

export function applyFactionProfile(id: string): Promise<FactionApplyResult> {
  return invoke<FactionApplyResult>("fe_profile_apply", { id, versionName: versionName() });
}

export function exportFactionProfile(id: string, destPath: string): Promise<void> {
  return invoke<void>("fe_profile_export", { id, destPath });
}

export function importFactionProfile(path: string): Promise<FactionProfileItem> {
  return invoke<FactionProfileItem>("fe_profile_import", { path });
}

export function updateFactionProfileMeta(id: string, name: string, description: string): Promise<FactionProfileItem> {
  return invoke<FactionProfileItem>("fe_profile_update_meta", { id, name, description });
}

export function deleteFactionProfile(id: string): Promise<void> {
  return invoke<void>("fe_profile_delete", { id });
}

export function factionProfilesDir(): Promise<string> {
  return invoke<string>("fe_profiles_dir");
}

/** Persist the version this screen works with. */
export function setFactionVersion(name: string | undefined): Promise<void> {
  return invoke<void>("fe_set_version", { versionName: name ?? null });
}

/** File-name-safe stem for a bundle: `Жёсткий баланс v2` -> `жёсткий_баланс_v2_2026-09-15`. */
export function factionBundleFileName(name: string): string {
  const slug =
    name
      .trim()
      .toLowerCase()
      .replace(/[^a-zа-яё0-9]+/gi, "_")
      .replace(/^_+|_+$/g, "") || "faction_settings";
  const date = new Date().toISOString().slice(0, 10);
  return `${slug}_${date}.gwfe`;
}

const KNOWN_ERROR_CODES = [
  "FE_ERR_NO_VERSION",
  "FE_ERR_NO_CONFIG",
  "FE_ERR_GAME_RUNNING",
  "FE_ERR_EXPORT_NOT_GWFE",
  "FE_ERR_BUNDLE_NOT_ZIP",
  "FE_ERR_BUNDLE_NO_MANIFEST",
  "FE_ERR_BUNDLE_KIND",
  "FE_ERR_BUNDLE_SCHEMA",
  "FE_ERR_BUNDLE_UNKNOWN_FILE",
  "FE_ERR_BUNDLE_TOO_LARGE",
  "FE_ERR_BUNDLE_HASH_MISMATCH",
  "FE_ERR_BUNDLE_INVALID_CONFIG",
  "FE_ERR_BUNDLE_INVALID_AXR_KEYS",
  "FE_ERR_APPLY_FAILED",
  "FE_ERR_ROLLBACK_FAILED",
  "FE_ERR_PROFILE_EXISTS",
  "FE_ERR_PROFILE_NAME_INVALID",
  "FE_ERR_PROFILE_NOT_FOUND",
];

const KNOWN_WARNING_CODES = ["FE_WARN_AXR_OPTIONS_MISSING", "FE_WARN_AXR_OPTIONS_SECTION_MISSING", "FE_WARN_UNKNOWN_FACTIONS"];

function errorText(err: unknown): string {
  return typeof err === "string" ? err : String((err as any)?.message ?? err ?? "");
}

/** The `FE_ERR_*` code an error starts with (the backend always puts the code
 * first: `bail!("{}: …", CODE)`), or `undefined`. */
export function factionErrorCode(err: unknown): string | undefined {
  const text = errorText(err);
  return KNOWN_ERROR_CODES.find((c) => text.startsWith(c));
}

export function isFactionGameRunningError(err: unknown): boolean {
  return factionErrorCode(err) === "FE_ERR_GAME_RUNNING";
}

/** Locale key under `app.factionSettings.errors.*` for a backend error. */
export function factionErrorKey(err: unknown): string {
  const code = factionErrorCode(err);
  return code ? `app.factionSettings.errors.${code}` : "app.factionSettings.errors.unknown";
}

/** Text after `CODE: ` in a backend error/warning, or the whole text when
 * there is no known code prefix — the raw detail to show under a localized
 * headline. */
export function factionMessageDetail(text: string): string {
  const code = [...KNOWN_ERROR_CODES, ...KNOWN_WARNING_CODES].find((c) => text.startsWith(c));
  if (!code) return text;
  const rest = text.slice(code.length);
  return rest.startsWith(": ") ? rest.slice(2) : "";
}

/** `FE_WARN_*` warning code -> locale key under `app.factionSettings.warnings.*`. */
export function factionWarningKey(warning: string): string {
  const code = KNOWN_WARNING_CODES.find((c) => warning.startsWith(c));
  return code ? `app.factionSettings.warnings.${code}` : "app.factionSettings.warnings.unknown";
}
