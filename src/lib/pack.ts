import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { progress } from '../store/pack';

let unlisten: (() => void) | null = null;

export async function initPackListener() {
  if (unlisten) return;

  unlisten = await listen('pack_archive_progress', (event: Event<number>) => {
    progress.set(event.payload);
  });
}
