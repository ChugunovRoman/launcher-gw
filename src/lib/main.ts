import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { fontColor, connectStatus, providersWasInited, allowPackMod, versionsWillBeLoaded, appConfig, fetchLocalVersions, showDlgRestartApp, newLauncherVersionDownloaded } from '../store/main';
import { ConnectStatus } from "../consts";
import { versions, updateEachVersion, hasAnyLocalVersion } from '../store/upload';
import { get } from 'svelte/store';
import { sep } from '@tauri-apps/api/path';

const unlisten: Map<string, (() => void)> = new Map();

export async function initMainListeners() {
  unlisten.set('background-init-success', await listen('background-init-success', (event: Event<void>) => {
    console.log("background-init-success !");

    providersWasInited.set(true);
    connectStatus.set(ConnectStatus.Connnected);
    fontColor.set("rgba(69, 240, 97, 1)");
  }));
  unlisten.set('background-init-failed', await listen('background-init-failed', (event: Event<string>) => {
    console.log("background-init-failed !, error: ", event.payload);

    providersWasInited.set(false);
    connectStatus.set(ConnectStatus.ConnnectError);
    fontColor.set("rgba(254, 197, 208, 1)");
  }));
  unlisten.set('user-data-loaded', await listen('user-data-loaded', (event: Event<void>) => {
    console.log("user-data-loaded ! ");

    invoke<boolean>("allow_pack_mod").then((value) => allowPackMod.set(value));
  }));
  unlisten.set('versions-loaded', await listen('versions-loaded', async (event: Event<Version[]>) => {
    console.log("versions-loaded ! payload: ", event.payload);

    const { default_download_path, default_installed_path } = get(appConfig);
    const separ = await sep();

    versions.set(event.payload.map(version => {
      return {
        ...version,
        installed_path: version.installed_path === "" ? `${default_installed_path}${separ}${version.path}` : version.installed_path,
        download_path: version.download_path === "" ? `${default_download_path}${separ}${version.path}_data` : version.download_path,
        is_local: false,
        inProgress: false,
        isStoped: false,
        downloadedFileBytes: 0,
        downloadSpeed: 0.0,
        downloadCurrentFile: "",
        downloadProgress: 0.0,
        downloadedFilesCnt: 0,
        totalFileCount: 0,
        speedValue: 0,
        sfxValue: "",
      }
    }));
    versionsWillBeLoaded.set(true);
  }));
  unlisten.set('launcher-new-version', await listen('launcher-new-version', (event: Event<string>) => {
    console.log('launcher-new-version:', event.payload);
    newLauncherVersionDownloaded.set(event.payload);
  }));

  unlisten.set('config-loaded', await listen('config-loaded', (event: Event<AppConfig>) => {
    invoke<boolean>('update').then(value => {
      console.log('launcher update:', value);
      showDlgRestartApp.set(value);
    });
    console.log("config-loaded ! payload: ", event.payload);
    appConfig.set(event.payload);
    const progressDownloads = event.payload.progress_download;

    fetchLocalVersions();

    updateEachVersion((version => {
      const progress = progressDownloads[version.name];
      if (progress) {
        return {
          ...version,
          installed_path: progress.installed_path,
          download_path: progress.download_path,
          inProgress: false,
          isStoped: true,
          downloadedFileBytes: 0,
          downloadSpeed: 0.0,
          downloadCurrentFile: "",
          downloadProgress: 0.0,
          downloadedFilesCnt: progress.downloaded_files_cnt,
          totalFileCount: progress.total_file_count,
          speedValue: 0,
          sfxValue: "",
        }
      }

      return version;
    }));
  }));
}
