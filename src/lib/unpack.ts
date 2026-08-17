import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { progress } from '../store/unpack';
import { updateVersionProgress } from '../store/upload';
import { DownloadStatus } from '../consts';

const unlistens: UnlistenFn[] = [];

export async function initUnpackListener() {
  if (unlistens.length > 0) return;

  unlistens.push(
    await listen('unpack_archive_progress', (event: Event<[string, number]>) => {
      const [versionName, percent] = event.payload;
      progress.set(percent);

      if (versionName !== "") {
        updateVersionProgress(versionName, () => ({
          status: DownloadStatus.Unpacking,
          downloadProgress: percent,
        }));
      }
    })
  );

  unlistens.push(
    await listen('game-archive-unack-progress', (event: Event<[string, string, number, number]>) => {
      const [versionName, fileName, size, total] = event.payload;
      const percent = total > 0 ? (size / total) * 100 : 0;
      const status = percent >= 100 ? 3 : 2;

      if (versionName !== "") {
        updateVersionProgress(versionName, (version) => {
          const map = version.filesProgress;
          const prev = map.get(fileName);
          if (!prev) {
            return {};
          }

          map.set(fileName, {
            ...prev,
            unpackProgress: percent,
            status,
          });

          return {
            filesProgress: map,
          };
        });
      }
    })
  );
}
