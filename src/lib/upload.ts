import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { logText, uploadedFiles, totalFiles, manifest, uploadFilesMap } from '../store/upload';
import { formatSpeedBytesPerSec } from '../utils/dwn';

const unlisten: Map<string, (() => void)> = new Map();

export async function initUploadListeners() {
  unlisten.set('upload-log', await listen('upload-log', (event: Event<string>) => {
    logText.push(event.payload);
  }));
  unlisten.set('upload-files-count', await listen('upload-files-count', (event: Event<number[]>) => {
    const [uploaded, total] = event.payload;
    totalFiles.set(total);
    uploadedFiles.set(uploaded);
  }));

  unlisten.set('upload-progress-get-manifest', await listen('upload-progress-get-manifest', (event: Event<ReleaseManifest>) => {
    manifest.set(event.payload);
    for (const file of event.payload.files) {
      uploadFilesMap.setItem(file.name, {
        file_uploaded_size: 0,
        file_total_size: file.size,
        progress: 0,
        speedValue: 0,
        sfxValue: "",
      });
    }
  }));
  unlisten.set('upload-progress', await listen('upload-progress', (event: Event<UploadProgressPayload>) => {
    const { file_name, total_size, total_uploaded_size, speed } = event.payload;

    const [speedValue, sfxValue] = formatSpeedBytesPerSec(speed);

    uploadFilesMap.setItem(file_name, {
      file_uploaded_size: total_uploaded_size,
      file_total_size: total_size,
      progress: total_uploaded_size / total_size * 100,
      speedValue,
      sfxValue,
    });
  }));
}
