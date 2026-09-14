import { invoke } from "@tauri-apps/api/core";
import { providers, versionsWillBeLoaded } from "../store/main";
import { loadVersions } from "./versions";

/**
 * Switch the current API provider and refresh the version list.
 * Awaits set_current_api_provider BEFORE fetching versions to avoid stale cache.
 */
export async function switchProvider(id: string): Promise<void> {
  // REGR-1: Reset loading flag so the UI shows a spinner during the switch.
  versionsWillBeLoaded.set(false);

  try {
    // Switch backend first — must complete before get_available_versions
    // to avoid getting the old provider's cached release list.
    await invoke("set_current_api_provider", { provider: id });

    await loadVersions();
  } catch (e) {
    console.error("switchProvider failed:", e);
    // Keep the existing version list on failure — do not clear it.
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
