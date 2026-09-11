import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { allowPackMod, versionsWillBeLoaded, appConfig, startupState, showDlgRestartApp, newLauncherVersionDownloaded, launcherDwnVersion, launcherDwnNeedUpdate, providers, moveProgress, gameStatus, showDlgTempPathWarning, tempPathWarningCodes, tempPathWarningKind } from '../store/main';
import { DownloadStatus } from "../consts";
import { versions } from '../store/upload';
import { get } from 'svelte/store';
import { sep } from '@tauri-apps/api/path';
import { getVersion } from '@tauri-apps/api/app';

const unlisten: Map<string, (() => void)> = new Map();

/// Idempotent launcher update check: fires once when providers reach Ok —
/// from the startup-state event OR from bootstrap() reading an already-final
/// state via get_startup_state (whichever happens first).
let updateCheckStarted = false;
export function maybeStartUpdateCheck() {
  if (updateCheckStarted) return;
  if (get(startupState).providers.status !== "ok") return;
  updateCheckStarted = true;

  invoke<boolean>('update').then(value => {
    console.log('launcher update:', value);
    showDlgRestartApp.set(value);
    if (!value) {
      getVersion().then(version => {
        launcherDwnNeedUpdate.set(false);
        launcherDwnVersion.set(version);
      });
    }
  }).catch(e => console.error("update check failed:", e));
}

/// Tauri rejects invoke promises with arbitrary values (string, object, ...).
/// Normalize anything into the backend's { code, detail } shape.
export function normalizeLaunchError(e: unknown): LaunchError {
  if (e && typeof e === "object" && "code" in e && typeof (e as any).code === "string") {
    return {
      code: (e as any).code,
      detail: String((e as any).detail ?? ""),
    };
  }
  return { code: "unknown", detail: String(e) };
}

/// Shared warning flow for install paths inside temp directories.
export async function warnIfTempPath(path: string) {
  try {
    const codes = await invoke<string[]>("check_install_path", { path });
    if (codes.length > 0) {
      tempPathWarningKind.set("install");
      tempPathWarningCodes.set(codes);
      showDlgTempPathWarning.set(true);
    }
  } catch (e) {
    console.error("check_install_path failed:", e);
  }
}

export async function initMainListeners() {
  // startup-state: aggregate phase of all startup sub-tasks.
  // connectStatus and fontColor are now derived from this store.
  unlisten.set('startup-state', await listen<StartupState>('startup-state', (event) => {
    console.log("startup-state:", event.payload);
    startupState.set(event.payload);

    // Launcher update check fires once when providers reach Ok.
    maybeStartUpdateCheck();
  }));

  // Compatibility: background-init-success/failed are still emitted by the
  // backend for one release cycle.  We no longer gate on them.
  unlisten.set('background-init-success', await listen('background-init-success', () => {
    console.log("background-init-success (compat)");
  }));
  unlisten.set('background-init-failed', await listen('background-init-failed', (event: Event<string>) => {
    console.log("background-init-failed (compat):", event.payload);
  }));

  unlisten.set('user-data-loaded', await listen('user-data-loaded', () => {
    console.log("user-data-loaded!");
    invoke<boolean>("allow_pack_mod").then((value) => allowPackMod.set(value));
  }));
  unlisten.set('game-status', await listen('game-status', (event: Event<GameStatus>) => {
    gameStatus.set(event.payload);
  }));
  unlisten.set('launcher-in-temp-dir', await listen('launcher-in-temp-dir', (event: Event<string[]>) => {
    console.warn("launcher runs from a temp dir:", event.payload);
    tempPathWarningKind.set("launcher");
    tempPathWarningCodes.set(event.payload);
    showDlgTempPathWarning.set(true);
  }));
  unlisten.set('versions-loaded', await listen('versions-loaded', async (event: Event<Version[]>) => {
    console.log("versions-loaded:", event.payload);
    const cfg = get(appConfig);
    // If appConfig is not yet populated (should not happen with bootstrap,
    // but guard against race), fetch it first.
    if (!cfg.default_installed_path) {
      try {
        const fresh = await invoke<AppConfig>('get_config');
        appConfig.set(fresh);
      } catch (e) {
        console.error("versions-loaded: get_config fallback failed", e);
      }
    }
    const separ = await sep();
    versions.set(event.payload.map(version => prepareVersionItem(get(appConfig), version, separ)));
    versionsWillBeLoaded.set(true);
  }));
  unlisten.set('providers-stats', await listen('providers-stats', () => {
    // Refresh the providers store from the backend.
    invoke<[string, ProviderStatus][]>('get_api_providers_stats').then(result => {
      result.sort((a, b) => (a[1].latency_ms ?? Number.MAX_SAFE_INTEGER) - (b[1].latency_ms ?? Number.MAX_SAFE_INTEGER));
      providers.set(result);
    }).catch(e => console.error("providers-stats refresh failed:", e));
  }));
  unlisten.set('launcher-new-version', await listen('launcher-new-version', (event: Event<string>) => {
    console.log('launcher-new-version:', event.payload);
    newLauncherVersionDownloaded.set(event.payload);
  }));
  unlisten.set('move-version', await listen('move-version', (event: Event<ProgressPayload>) => {
    const { version_name } = event.payload;
    moveProgress.setItem(version_name, event.payload);
  }));
}

export function prepareVersionItem(appConfig: AppConfig, version: Version, sep: string): Version {
  const { default_download_path, default_installed_path, progress_download } = appConfig;
  const progress = progress_download[version.name];
  let installed_path = version.installed_path === "" ? `${default_installed_path}${sep}${version.path}` : version.installed_path;
  let download_path = version.download_path === "" ? `${default_download_path}${sep}${version.path}_data` : version.download_path;
  let downloadProgress = 0.0;
  let downloadedFilesCnt = 0;
  let totalFileCount = 0;
  let isStoped = false;
  let status = DownloadStatus.Init;

  if (progress) {
    installed_path = progress.installed_path;
    download_path = progress.download_path;
    downloadedFilesCnt = progress.downloaded_files_cnt;
    totalFileCount = progress.total_file_count;
    isStoped = true;
    downloadProgress = (downloadedFilesCnt / totalFileCount) * 100.0;
    status = DownloadStatus.Pause;

    invoke('emit_file_list_stats', { versionName: version.name });
  }

  return {
    ...version,
    installed_path,
    download_path,
    is_local: false,
    inProgress: false,
    wasCanceled: false,
    isStoped,
    downloadedFileBytes: 0,
    downloadSpeed: 0.0,
    downloadCurrentFile: "",
    downloadProgress,
    downloadedFilesCnt,
    totalFileCount,
    speedValue: 0,
    sfxValue: "",
    filesProgress: new Map(),
    status,
  }
}
