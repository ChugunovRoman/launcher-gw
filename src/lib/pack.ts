import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { status, currentFile, processedSize, progress, totalSize } from '../store/pack';

const unlistens: UnlistenFn[] = [];

export async function initPackListener() {
  if (unlistens.length > 0) return;

  unlistens.push(
    await listen('packing-progress', (event: Event<CompressProgressPayload>) => {
      status.set(event.payload.status);
      currentFile.set(event.payload.current_file);
      totalSize.set(event.payload.total_size);
      processedSize.set(event.payload.processed_size);
      progress.set(event.payload.percentage);
    })
  );
}
