import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { status, currentFile, processedSize, progress, totalSize } from '../store/pack';

let unlisten: (() => void) | null = null;

export async function initPackListener() {
  if (unlisten) return;

  unlisten = await listen('pack_archive_progress', (event: Event<number>) => {
    progress.set(event.payload);
  });
  unlisten = await listen('packing-progress', (event: Event<CompressProgressPayload>) => {
    status.set(event.payload.status);
    currentFile.set(event.payload.current_file);
    totalSize.set(event.payload.total_size);
    processedSize.set(event.payload.processed_size);
    progress.set(event.payload.percentage);
  });
}
