import { writable } from "svelte/store";

/** Game version this screen reads settings from and applies them to. Empty
 * string means "let the backend fall back to the launch-time version". Kept
 * separate from the version selected for launching on purpose. */
export const factionVersionName = writable<string>("");

export const factionProfiles = writable<FactionProfileItem[]>([]);
export const factionSelectedId = writable<string | undefined>(undefined);
export const factionBusy = writable(false);
export const factionOpError = writable<string>("");

// "Save current settings as a new profile" / "Export current settings" — the
// same fields (name/description/author), two different destinations.
export type FactionMetaMode = "save" | "exportCurrent";
export const showDlgFactionMeta = writable(false);
export const factionMetaMode = writable<FactionMetaMode>("save");

// Rename/describe an existing profile (id/name never change together with content).
export const showDlgFactionRename = writable(false);
export const factionRenameTarget = writable<{ id: string; name: string; description: string } | undefined>(undefined);

export const showDlgFactionDelete = writable(false);
export const factionDeleteTarget = writable<{ id: string; name: string } | undefined>(undefined);

// "Вы действительно хотите заменить настройки в редакторе фракций?" — shared
// by importing a file, applying a stored profile, and resetting to defaults.
// `warnings` come from `inspect` against the local install (unknown factions).
export type FactionApplyContext =
  | { kind: "importFile"; path: string; manifest: FactionBundleManifest; warnings: string[] }
  | { kind: "applyProfile"; id: string; manifest: FactionBundleManifest; warnings: string[] }
  | { kind: "resetDefault" };
export const showDlgFactionApplyConfirm = writable(false);
export const factionApplyContext = writable<FactionApplyContext | undefined>(undefined);

export const showDlgFactionGameRunning = writable(false);

export const showDlgFactionResult = writable(false);
export const factionResult = writable<{ ok: boolean; message: string; detail?: string; warnings: string[]; backupPath: string | null } | undefined>(
  undefined,
);
