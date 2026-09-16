import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { selectedVersion, updateVersionProgress, removeDownloadState, versions } from '../store/upload';
import { formatSpeedBytesPerSec } from '../utils/dwn';
import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { expandedKey, fetchLocalVersions, launcherDwnBytes, launcherDwnNeedUpdate, launcherDwnProgress, launcherDwnTotalBytes, launcherDwnVersion, localVersions } from '../store/main';

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

      // Every finished file reports speed 0, and while a file is being hashed
      // or unpacked no speed events arrive at all — letting those zeroes reach
      // the screen showed "0 B/s" for minutes in the middle of an active
      // download. The aggregate itself keeps tracking the real value (so the
      // next incremental delta stays correct); only the DISPLAYED value holds
      // the last non-zero reading. It is zeroed again by the UI as soon as the
      // version leaves the in-progress state.
      const [rawSpeedValue, rawSfxValue] = formatSpeedBytesPerSec(totalSpeed);
      const totalSpeedValue = totalSpeed > 0 ? rawSpeedValue : (version.speedValue ?? 0);
      const totalSfxValue = totalSpeed > 0 ? rawSfxValue : (version.sfxValue ?? "");

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
  unlisten.set('download-version-file-verify', await listen('download-version-file-verify', (event: Event<[string, string, number, number]>) => {
    const [versionName, fileName, doneBytes, totalBytes] = event.payload;

    updateVersionProgress(versionName, (version) => {
      const map = version.filesProgress ?? new Map();
      const prev = map.get(fileName);
      if (!prev) {
        return {};
      }

      map.set(fileName, {
        ...prev,
        downloadProgress: 100,
        status: 4,
      });

      return {
        filesProgress: map,
      };
    });
  }));
  unlisten.set('download-version-file-error', await listen('download-version-file-error', (event: Event<FileErrorPayload>) => {
    const { version_name, file, code } = event.payload;

    updateVersionProgress(version_name, (version) => {
      const map = version.filesProgress ?? new Map();
      const prev = map.get(file);
      if (prev) {
        map.set(file, {
          ...prev,
          downloadProgress: 0,
          status: 5,
          errorCode: code,
        });
      }

      // Do NOT flip the version-level status here: other files may still be
      // downloading (the queue keeps going past one failed file, plan Q6),
      // and DownloadStatus.Error would hide the Pause/Stop buttons while the
      // version is genuinely still in progress. The version-level Error
      // status + inProgress:false transition is set once, at the end, by the
      // start/continue/repair command's DOWNLOAD_FAILED catch handler.
      return {
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
      // The payload is the AUTHORITATIVE file list of the version: the backend
      // drops files that are no longer part of the release. Rebuilding the map
      // from it (instead of merging into the old one) is what removes them
      // here too — carrying the stale entries over kept counting their bytes
      // against the new, smaller total size ("Downloaded: 108.42%").
      const prevMap = version.filesProgress ?? new Map();
      const map = new Map<string, VersionFileDownload>();
      const totals = new Map((version.manifest?.files || []).map((f) => [f.name, f.size]));

      for (const item of fileSizesMap) {
        const old = prevMap.get(item.name);
        const totalFileBytes = old?.totalFileBytes || totals.get(item.name) || 0;
        const downloadedFileBytes = item.size || 0;
        map.set(item.name, {
          downloadProgress: totalFileBytes > 0 ? (downloadedFileBytes / totalFileBytes) * 100 : 0,
          downloadedFileBytes,
          totalFileBytes,
          // `unpacked: false` must be able to CLEAR a previous "done" state,
          // not just fail to set it. The backend resets is_unpacked whenever a
          // file goes back into the queue (a re-published release, a failed
          // re-verify), and carrying the old status forward left a green check
          // sitting next to a file that was downloading again — exactly what
          // players reported.
          unpackProgress: item.unpacked ? 100 : (old?.unpackProgress === 100 ? 0 : (old?.unpackProgress ?? 0)),
          downloadSpeed: old?.downloadSpeed || 0,
          speedValue: old?.speedValue || 0,
          sfxValue: old?.sfxValue || "",
          status: item.unpacked ? 3 : (old?.status === 3 ? 0 : (old?.status ?? 0)),
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
  unlisten.set('file-unzipped', await listen('file-unzipped', (event: Event<[string, string | null]>) => {
    // Fired after unzip OR after a raw file was copied into the install dir —
    // the file is fully post-processed. Payload carries the archive path.
    const [versionName, archivePath] = event.payload;
    if (!archivePath) return;
    const fileName = archivePath.split(/[\\/]/).pop() ?? archivePath;

    updateVersionProgress(versionName, (version) => {
      const map = version.filesProgress ?? new Map();
      const prev = map.get(fileName);
      if (!prev) {
        return {};
      }

      map.set(fileName, {
        ...prev,
        unpackProgress: 100,
        status: 3,
      });

      return {
        filesProgress: map,
      };
    });
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

    expandedKey.set(null);
  }));
}
