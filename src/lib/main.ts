import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { fontColor, connectStatus, providersWasInited, allowPackMod, versionsWillBeLoaded, appConfig, fetchLocalVersions, showDlgRestartApp, newLauncherVersionDownloaded } from '../store/main';
import { ConnectStatus, DownloadStatus } from "../consts";
import { versions } from '../store/upload';
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

    const { default_download_path, default_installed_path, progress_download } = get(appConfig);
    const separ = await sep();

    versions.set(event.payload.map(version => {
      const progress = progress_download[version.name];
      let installed_path = version.installed_path === "" ? `${default_installed_path}${separ}${version.path}` : version.installed_path;
      let download_path = version.download_path === "" ? `${default_download_path}${separ}${version.path}_data` : version.download_path;
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

    fetchLocalVersions();
  }));
}
