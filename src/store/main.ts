import { writable } from 'svelte/store';
import { createMapStore } from './helpers';
import { ConnectStatus } from '../consts';
import { invoke } from '@tauri-apps/api/core';
import { hasAnyLocalVersion, removeVersion } from './upload';

export const appConfig = writable<AppConfig>({
  default_installed_path: "",
  default_download_path: "",
} as any);

export const connectStatus = writable(ConnectStatus.Connnecting);
export const fontColor = writable("rgba(243, 240, 63, 1)");

export const providersWasInited = writable(false);
export const inDownloading = writable(false);
export const versionsWillBeLoaded = writable(false);

export const allowPackMod = writable(false);

export const newLauncherVersionDownloaded = writable("");
export const showDlgRestartApp = writable(false);

export const loadedTokens = writable(false);
export const tokens = writable<Map<string, string>>(new Map());

export const localVersions = createMapStore<string, Version>();

export function updateConfig<F extends keyof AppConfig>(field: F, value: any) {
  appConfig.update(cfg => {
    cfg[field] = value;

    return cfg;
  });
}

export async function fetchLocalVersions() {
  const [versions_1, versions_2] = await Promise.all([
    invoke<Version[]>("get_local_version"),
    invoke<Version[]>("get_installed_versions"),
  ]);

  const common = versions_1.concat(versions_2);
  for (const version of common) {
    localVersions.setItem(version.name, version);
    removeVersion(version.name);
  }

  if (common.length) {
    hasAnyLocalVersion.set(true);
  }
}
