import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { selectedVersion, updateVersionProgress, removeDownloadState, versions } from '../store/upload';
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

    updateVersionProgress(version_name, () => ({
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

    updateVersionProgress(versionName, (version) => {
      // Mutate the EXISTING filesProgress map (single entry) instead of
      // copying the whole manifest map per event — O(1) instead of O(files),
      // which froze the UI on manifests with thousands of files. The version
      // object itself is still replaced by updateVersionProgress, so the UI
      // refreshes as before.
      const map = version.filesProgress ?? new Map();
      const prev = map.get(fileName);
      const prevBytes = prev?.downloadedFileBytes ?? 0;
      const prevSpeed = prev?.downloadSpeed ?? 0;

      map.set(fileName, {
        downloadProgress: totalBytes > 0 ? (bytes / totalBytes) * 100 : 0,
        unpackProgress: 0,
        downloadedFileBytes: bytes,
        totalFileBytes: totalBytes,
        downloadSpeed: speed,
        speedValue,
        sfxValue,
        status: 1,
      });

      // Incremental aggregates: adjust the totals by this file's delta
      // instead of re-iterating the whole map.
      const downloadFilesTotalBytes = (version.downloadedFileBytes ?? 0) - prevBytes + bytes;
      const totalSpeed = Math.max(0, (version.downloadSpeed ?? 0) - prevSpeed + speed);

      const [totalSpeedValue, totalSfxValue] = formatSpeedBytesPerSec(totalSpeed);

      let downloadProgressVersion = version.downloadProgress;

      if (version.manifest && version.manifest.compressed_size > 0) {
        downloadProgressVersion = (downloadFilesTotalBytes / version.manifest.compressed_size) * 100;
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
    launcherDwnProgress.set(totalSize > 0 ? (bytes / totalSize) * 100 : 0);
  }));
  unlisten.set('download-version-files', await listen('download-version-files', (event: Event<[string, { name: string; unpacked: boolean; size: number }[]]>) => {
    const [versionName, fileSizesMap] = event.payload;

    updateVersionProgress(versionName, (version) => {
      const map = new Map(version.filesProgress);
      const totals = new Map((version.manifest?.files || []).map((f) => [f.name, f.size]));

      for (const item of fileSizesMap) {
        const old = map.get(item.name);
        const totalFileBytes = old?.totalFileBytes || totals.get(item.name) || 0;
        const downloadedFileBytes = item.size || 0;
        map.set(item.name, {
          downloadProgress: totalFileBytes > 0 ? (downloadedFileBytes / totalFileBytes) * 100 : 0,
          downloadedFileBytes,
          totalFileBytes,
          unpackProgress: item.unpacked ? 100 : (old?.unpackProgress || 0),
          downloadSpeed: old?.downloadSpeed || 0,
          speedValue: old?.speedValue || 0,
          sfxValue: old?.sfxValue || "",
          status: item.unpacked ? 3 : (old?.status || 0),
        });
      }

      for (const file of version.manifest?.files || []) {
        if (!map.has(file.name)) {
          map.set(file.name, {
            downloadProgress: 0,
            downloadedFileBytes: 0,
            totalFileBytes: file.size,
            unpackProgress: 0,
            downloadSpeed: 0,
            speedValue: 0,
            sfxValue: "",
            status: 0,
          });
        }
      }

      // Re-sync the version-level aggregate with the rebuilt map. The speed
      // handler (download-speed-status) updates it INCREMENTALLY from this
      // base — without the resync a resumed download restarted the total
      // progress bar from 0 even though most bytes were already on disk.
      let downloadedFilesTotalBytes = 0;
      for (const progress of map.values()) {
        downloadedFilesTotalBytes += progress.downloadedFileBytes;
      }

      let downloadProgressVersion = version.downloadProgress;
      if (version.manifest && version.manifest.compressed_size > 0) {
        downloadProgressVersion = (downloadedFilesTotalBytes / version.manifest.compressed_size) * 100;
      }

      return {
        filesProgress: map,
        downloadedFileBytes: downloadedFilesTotalBytes,
        downloadProgress: downloadProgressVersion,
      };
    });
  }));
  unlisten.set('cancel-download-version', await listen('cancel-download-version', (event: Event<string>) => {
    const versionName = event.payload;

    console.log("cancel-download-version, versionName: ", versionName);
  }));
  unlisten.set('download-unpack-version', await listen('download-unpack-version', async (event: Event<string>) => {
    const versionName = event.payload;

    // Each post-unpack step is independent cleanup. A failure in one must not
    // abort the others — otherwise clear_progress_version never runs and the UI
    // hangs in the "in progress" state (see remove_download_version NotFound bug).
    try {
      await invoke<void>("add_installed_version_from_config", { versionName });
    } catch (e) {
      console.error("add_installed_version_from_config failed:", e);
    }

    try {
      await invoke<void>("remove_download_version", { versionName });
    } catch (e) {
      console.error("remove_download_version failed:", e);
    }

    try {
      await invoke<void>("clear_progress_version", { versionName });
    } catch (e) {
      console.error("clear_progress_version failed:", e);
    }

    removeDownloadState(versionName);

    if (localVersions.size() === 0) {
      selectedVersion.set(undefined);
    }

    await fetchLocalVersions();

    if (!get(selectedVersion)) {
      selectedVersion.set([...get(localVersions).keys()][0]);
      try {
        await invoke<void>("set_current_game_version", { versionName: get(selectedVersion) });
      } catch (e) {
        console.error("set_current_game_version failed:", e);
      }
    }

    expandedIndex.set(null);
  }));
}
