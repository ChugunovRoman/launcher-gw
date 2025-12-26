import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { progress } from '../store/unpack';
import { updateVersion } from '../store/upload';
import { DownloadStatus } from '../consts';

let unlisten: (() => void) | null = null;

export async function initUnpackListener() {
  if (unlisten) return;

  unlisten = await listen('unpack_archive_progress', (event: Event<[string, number]>) => {
    const [versionName, percent] = event.payload;
    progress.set(percent);

    if (versionName !== "") {
      updateVersion(versionName, () => ({
        status: DownloadStatus.Unpacking,
        downloadProgress: percent,
      }));
    }
  });
}
