import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { logText, uploadedFiles, totalFiles } from '../store/upload';

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
}
