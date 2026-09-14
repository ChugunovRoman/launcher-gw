import { derived, get, writable } from 'svelte/store';
import { createMapStore } from './helpers';
import { ConnectStatus } from '../consts';
import { invoke } from '@tauri-apps/api/core';
import { hasAnyLocalVersion } from './upload';

export const appConfig = writable<AppConfig>({
  default_installed_path: "",
  default_download_path: "",
} as any);

// Whether the local config has been loaded into stores (bootstrap() done).
export const configReady = writable(false);

// Aggregate startup state from the backend (providers, releases, user_data, profiles).
export const startupState = writable<StartupState>({
  providers: { status: "pending" },
  releases: { status: "pending" },
  user_data: { status: "pending" },
  profiles: { status: "pending" },
});

// Derived from startupState.providers — replaces the old hardcoded connectStatus/fontColor.
export const connectStatus = derived(startupState, ($s) => {
  if ($s.providers.status === "ok") return ConnectStatus.Connnected;
  if ($s.providers.status === "error") return ConnectStatus.ConnnectError;
  return ConnectStatus.Connnecting;
});
export const fontColor = derived(startupState, ($s) => {
  if ($s.providers.status === "ok") return "rgba(69, 240, 97, 1)";
  if ($s.providers.status === "error") return "rgba(254, 197, 208, 1)";
  return "rgba(243, 240, 63, 1)";
});

// Which version card is expanded, addressed by a STABLE key instead of a
// positional index: "local:<name>" for an installed version, "remote:<name>"
// for a release from the list.  A numeric index silently pointed at a
// different card whenever the list was replaced (refresh, provider switch).
export const expandedKey = writable<string | null>(null);
export const localKey = (name: string) => `local:${name}`;
export const remoteKey = (name: string) => `remote:${name}`;

export const versionsWillBeLoaded = writable(false);

export const allowPackMod = writable(false);

export const newLauncherVersionDownloaded = writable("");
export const showDlgRestartApp = writable(false);

export const removeVersion = writable<Version | undefined>();
export const removeVersionInProcess = writable(false);
export const showDlgRemoveVersion = writable(false);
export const showDlgAddVersion = writable(false);
export const showDlgMaxPerformancePresetWarning = writable(false);

export const loadedTokens = writable(false);
export const tokens = writable<Map<string, string>>(new Map());

export const launcherDwnNeedUpdate = writable(false);
export const launcherDwnVersion = writable<string | undefined>();
export const launcherDwnBytes = writable(0);
export const launcherDwnTotalBytes = writable(0);
export const launcherDwnProgress = writable(0);

// Game process state. Single source of truth is the backend GameTracker:
// the frontend only mirrors `game-status` events / `get_game_status` here.
export const gameStatus = writable<GameStatus>({ running: false, pid: null, version_name: null });

// Launch error dialog: backend returns { code, detail }.
export const showDlgLaunchError = writable(false);
export const launchError = writable<LaunchError | null>(null);

// Temp-path warning dialog ("install" = chosen install path, "launcher" = the
// launcher itself runs from a temp folder).
export const showDlgTempPathWarning = writable(false);
export const tempPathWarningCodes = writable<string[]>([]);
export const tempPathWarningKind = writable<"install" | "launcher">("install");

export const providers = writable<[string, ProviderStatus][]>([]);
export const radioApiProvider = writable<string>("github");

export const moveProgress = createMapStore<string, ProgressPayload>();

export const localVersions = createMapStore<string, Version>();

// Patch check & install stores
export const patchCheckResults = createMapStore<string, { count: number; checkedAt: number }>();
export const patchInstallProgress = writable<PatchInstallProgress | null>(null);
export const patchInstallLog = writable<string[]>([]);
export const showDlgPatchNotes = writable(false);
export const patchNotesData = writable<{ title: string; notes: string | null } | null>(null);

export function updateConfig<F extends keyof AppConfig>(field: F, value: any) {
  appConfig.update(cfg => {
    cfg[field] = value;

    return cfg;
  });
}

export function removeLocalVersion(name: string) {
  localVersions.update((data) => {
    data.delete(name);

    return data;
  });
}
export function updateLocalVersion(releaseName: string, cb: (data: Version) => Partial<Version>) {
  localVersions.update((data) => {
    if (data.has(releaseName)) {
      data.set(releaseName, {
        ...data.get(releaseName)!,
        ...cb(data.get(releaseName)!),
      });
    }
    return data;
  });
}
export function refreshLocalVersion() {
  localVersions.set(get(localVersions));
}

export async function fetchLocalVersions() {
  const [versions_1, versions_2] = await Promise.all([
    invoke<Version[]>("get_local_version"),
    invoke<Version[]>("get_installed_versions"),
  ]);

  for (const version of versions_2) {
    localVersions.setItem(version.name, version);
  }
  for (const version of versions_1) {
    const found = [...get(localVersions).values()].find(v => v.installed_path === version.installed_path);
    if (!found) {
      localVersions.setItem(version.name, version);
    }
  }

  if (versions_1.length || versions_2.length) {
    hasAnyLocalVersion.set(true);
  }
}
