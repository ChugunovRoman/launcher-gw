import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { selectedVersion, updateVersion, versions } from '../store/upload';
import { formatSpeedBytesPerSec } from '../utils/dwn';
import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { expandedIndex, fetchLocalVersions, launcherDwnBytes, launcherDwnNeedUpdate, launcherDwnProgress, launcherDwnTotalBytes, launcherDwnVersion, localVersions } from '../store/main';

const unlisten: Map<string, (() => void)> = new Map();

export async function initDownloadListeners() {
  unlisten.set('download-version', await listen('download-version', (event: Event<DownloadProgress>) => {
    const {
      version_name,
      status,
      file,
      progress,
      downloaded_files_cnt,
      total_file_count,
    } = event.payload;

    updateVersion(version_name, () => ({
      downloadCurrentFile: file,
      downloadProgress: progress,
      downloadedFilesCnt: downloaded_files_cnt,
      totalFileCount: total_file_count,
      status,
    }));
  }));
  unlisten.set('download-speed-status', await listen('download-speed-status', (event: Event<[string, string, number, number, number]>) => {
    const [versionName, fileName, bytes, totalBytes, speed] = event.payload;

    const [speedValue, sfxValue] = formatSpeedBytesPerSec(speed);

    updateVersion(versionName, (version) => {
      const map = version.filesProgress;
      let totalSpeed = 0;
      let downloadFilesTotalBytes = 0;

      map.set(fileName, {
        downloadProgress: bytes / totalBytes * 100,
        unpackProgress: 0,
        downloadedFileBytes: bytes,
        totalFileBytes: totalBytes,
        downloadSpeed: speed,
        speedValue,
        sfxValue,
      });

      for (const [name, progress] of map) {
        totalSpeed += progress.downloadSpeed;
        downloadFilesTotalBytes += progress.downloadedFileBytes;
      }

      const [totalSpeedValue, totalSfxValue] = formatSpeedBytesPerSec(totalSpeed);

      let downloadProgressVersion = version.downloadProgress;

      if (version.manifest) {
        downloadProgressVersion = downloadFilesTotalBytes / version.manifest.compressed_size * 100;
      }

      return {
        downloadProgress: downloadProgressVersion,
        downloadedFileBytes: downloadFilesTotalBytes,
        downloadSpeed: totalSpeed,
        speedValue: totalSpeedValue,
        sfxValue: totalSfxValue,
        filesProgress: map,
      };
    });
  }));
  unlisten.set('download-launcher-status', await listen('download-launcher-status', (event: Event<[string, number, number]>) => {
    const [versionName, bytes, totalSize] = event.payload;

    launcherDwnNeedUpdate.set(true);
    launcherDwnVersion.set(versionName);
    launcherDwnBytes.set(bytes);
    launcherDwnTotalBytes.set(totalSize);
    launcherDwnProgress.set(bytes / totalSize * 100);
  }));
  unlisten.set('download-version-files', await listen('download-version-files', (event: Event<[string, { name: string; unpacked: boolean; size: number }[]]>) => {
    const [versionName, fileSizesMap] = event.payload;

    updateVersion(versionName, (version) => {
      const map = new Map();

      for (const item of fileSizesMap) {
        const old = version.filesProgress.get(item.name);
        map.set(item.name, {
          downloadProgress: old ? (old.downloadedFileBytes / old?.totalFileBytes || 0) * 100 : 0,
          downloadedFileBytes: item.size || 0,
          totalFileBytes: old?.totalFileBytes || 0,
          unpackProgress: item.unpacked ? 100 : 0,
          downloadSpeed: 0,
          speedValue: 0,
          sfxValue: "",
        });
      }

      return {
        filesProgress: map,
      };
    });
  }));
  unlisten.set('cancel-download-version', await listen('cancel-download-version', (event: Event<string>) => {
    const versionName = event.payload;

    console.log("cancel-download-version, versionName: ", versionName);
  }));
  unlisten.set('download-unpack-version', await listen('download-unpack-version', async (event: Event<string>) => {
    const versionName = event.payload;

    await invoke<void>("add_installed_version_from_config", { versionName });

    await invoke<void>("remove_download_version", { versionName });

    await invoke<void>("clear_progress_version", { versionName });

    if (localVersions.size() === 0) {
      selectedVersion.set(undefined);
    }

    await fetchLocalVersions();

    if (!get(selectedVersion)) {
      selectedVersion.set([...get(localVersions).keys()][0]);
    }

    expandedIndex.set(null);
  }));
}
