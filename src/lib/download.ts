import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { updateVersion, versions } from '../store/upload';
import { formatSpeedBytesPerSec } from '../utils/dwn';
import { invoke } from '@tauri-apps/api/core';
import { join } from '@tauri-apps/api/path';
import { get } from 'svelte/store';
import { fetchLocalVersions } from '../store/main';

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
  unlisten.set('download-speed-status', await listen('download-speed-status', (event: Event<[string, number, number]>) => {
    const [versionName, bytes, speed] = event.payload;

    const [speedValue, sfxValue] = formatSpeedBytesPerSec(speed);

    updateVersion(versionName, () => ({
      downloadedFileBytes: bytes,
      downloadSpeed: speed,
      speedValue,
      sfxValue,
    }));
  }));
  unlisten.set('download-unpack-version', await listen('download-unpack-version', async (event: Event<string>) => {
    const versionName = event.payload;

    const version = get(versions).find(v => v.name === versionName);

    if (!version) {
      throw new Error(`version by name: ${versionName} not found !`);
    }

    await invoke<string>("extract_archive", {
      versionName,
      archivePath: await join(version.download_path, "game.7z.001"),
      outputDir: await join(version.installed_path),
    });

    await invoke<void>("add_installed_version_from_config", { versionName });

    await invoke<void>("remove_download_version", { versionName });

    await invoke<void>("clear_progress_version", { versionName });

    await fetchLocalVersions();
  }));
}
