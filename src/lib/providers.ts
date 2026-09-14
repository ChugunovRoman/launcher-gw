import { invoke } from "@tauri-apps/api/core";
import { providers, versionsWillBeLoaded } from "../store/main";
import { loadVersions } from "./versions";

/// Provider id a *backend-initiated* `radioApiProvider` update is expected to
/// deliver (e.g. the `provider-fallback` event), so the Settings `$effect` can
/// skip the redundant `switchProvider` call (R7 fix).
///
/// R11: this used to be a plain boolean, and only the effect ever cleared it.
/// `provider-fallback` fires on startup while Settings is usually not mounted,
/// so the flag stayed `true` and silently swallowed the first *real* user
/// switch. Storing the expected value instead means a foreign effect run can no
/// longer eat the flag: only the exact value the backend announced is skipped.
let _backendSwitchValue: string | null = null;
export function getBackendProviderSwitch() { return _backendSwitchValue; }
export function setBackendProviderSwitch(v: string | null) { _backendSwitchValue = v; }

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
    throw e;
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
