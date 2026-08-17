import { invoke } from "@tauri-apps/api/core";
import { sep } from "@tauri-apps/api/path";
import { appConfig, providers, providersWasInited, radioApiProvider, versionsWillBeLoaded } from "../store/main";
import { downloadStates, restoreDownloadState, versions } from "../store/upload";
import { prepareVersionItem } from "./main";
import { get } from "svelte/store";

import { localVersions } from "../store/main";

function hasLocalVersion(version: Version) {
  for (const [name, local] of get(localVersions)) {
    if (name === version.name) return true;
    if (local.path === version.name) return true;
    if (local.path === version.path) return true;
  }
  return false;
}

/**
 * Switch the current API provider and refresh the version list.
 * Awaits set_current_api_provider BEFORE fetching versions to avoid stale cache.
 * Restores download progress from the overlay for versions present on the new provider.
 */
export async function switchProvider(id: string): Promise<void> {
  versionsWillBeLoaded.set(false);

  try {
    // Switch backend first — must complete before get_available_versions
    // to avoid getting the old provider's cached release list.
    await invoke("set_current_api_provider", { provider: id });

    const data = await invoke<Version[]>("get_available_versions");
    const separ = await sep();

    versions.set(
      data
        .map((version) => {
          const prepared = prepareVersionItem(get(appConfig), version, separ);
          // Restore JS-only download fields if we have a saved progress state.
          return restoreDownloadState(prepared);
        })
        .filter((v) => !hasLocalVersion(v))
    );
  } catch (e) {
    console.error("switchProvider failed:", e);
    // Keep the existing version list on failure — do not clear it.
  } finally {
    versionsWillBeLoaded.set(true);
  }
}

/**
 * Ping a single provider by id. Updates the providers store with fresh status.
 */
export async function pingProvider(id: string): Promise<ProviderStatus> {
  const result = await invoke<[string, ProviderStatus]>("ping_api_provider", { providerId: id });

  // Update the providers store with the fresh status.
  providers.update((list) => {
    return list.map(([pid, stats]) => {
      if (pid === id) return [pid, result[1]];
      return [pid, stats];
    });
  });

  return result[1];
}
