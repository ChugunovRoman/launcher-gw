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
  unlisten = await listen('game-archive-unack-progress', (event: Event<[string, string, number, number]>) => {
    const [versionName, fileName, size, total] = event.payload;

    const percent = size / total * 100;
    let status = 2;
    if (percent === 100) {
      status = 3;
    }

    console.log('game-archive-unack-progress, payload: ', percent, event.payload);

    if (versionName !== "") {
      updateVersion(versionName, (version) => {
        const map = version.filesProgress;

        map.set(fileName, {
          ...map.get(fileName)!,
          unpackProgress: percent,
          status,
        });

        return {
          filesProgress: map,
        };
      });
    }
  });
}
