import { listen } from "@tauri-apps/api/event";
import { patchCheckResults, patchInstallProgress, patchInstallLog } from "../store/main";

export async function initPatchListeners() {
  // Auto-check result: patches-available { version: string, count: number }
  await listen<[string, number]>("patches-available", (e) => {
    const [versionName, count] = e.payload;
    patchCheckResults.update((map) => {
      map.set(versionName, {
        count,
        checkedAt: Date.now(),
      });
      return map;
    });
    console.log(`[patches] ${count} patches available for '${versionName}'`);
  });

  // Install progress: PatchInstallProgress { stage, file, file_progress, total_progress }
  await listen<PatchInstallProgress>("patch-install-progress", (e) => {
    patchInstallProgress.set(e.payload);
  });

  // Install log messages
  await listen<string>("patch-install-log", (e) => {
    patchInstallLog.update((logs) => [...logs.slice(-50), e.payload]);
  });
}
