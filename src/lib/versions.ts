import { invoke } from "@tauri-apps/api/core";
import { sep } from "@tauri-apps/api/path";
import { get } from "svelte/store";
import { appConfig, versionsWillBeLoaded } from "../store/main";
import { restoreDownloadState, versions } from "../store/upload";
import { prepareVersionItem } from "./main";

// Generation counter: if a newer write starts before an older one finishes,
// the stale result is silently discarded (D5: race protection).
// EVERY writer of the `versions` store must go through loadVersions() or
// applyVersions() so they all share this counter — a direct versions.set()
// elsewhere can still clobber a newer list.
let gen = 0;
// Whether a list that came from the backend has already been applied.  The
// config cache read in bootstrap() and the `versions-loaded` event race each
// other; the cached copy must never replace fresher backend data, and the
// generation counter alone cannot tell the two apart (both are "already
// resolved" by the time they write).
let backendApplied = false;

/**
 * Write an already-fetched payload into the store with the shared transform
 * chain (prepareVersionItem -> restoreDownloadState).
 *
 * Used by the paths that receive the list without asking for it: the config
 * cache at startup and the backend's `versions-loaded` event.  Both count as
 * a new generation, so a load that is still in flight cannot overwrite them.
 */
export async function applyVersions(data: Version[], source: "backend" | "cache" = "backend"): Promise<void> {
  if (source === "cache" && backendApplied) return;

  const my = ++gen;
  const separ = await sep();
  if (my !== gen) return;

  versions.set(
    data.map((v) => restoreDownloadState(prepareVersionItem(get(appConfig), v, separ)))
  );
  if (source === "backend") backendApplied = true;
  versionsWillBeLoaded.set(true);
}

/**
 * Single entry point for loading/reloading the versions list.
 * Used by bootstrap, versions-loaded event, switchProvider, Releases,
 * and RemoveVersion — instead of each having its own pipeline.
 *
 * @param force  When true, calls `refresh_available_versions` (invalidates
 *               cache, re-fetches index).  When false, uses the normal
 *               `get_available_versions` path.
 * @throws Propagates errors so callers can display them in the UI.
 */
export async function loadVersions(force = false): Promise<void> {
  const my = ++gen;
  const cmd = force ? "refresh_available_versions" : "get_available_versions";
  const data = await invoke<Version[]>(cmd);
  // A newer load superseded this one — discard.
  if (my !== gen) return;

  const separ = await sep();
  if (my !== gen) return;

  versions.set(
    data.map((v) => restoreDownloadState(prepareVersionItem(get(appConfig), v, separ)))
  );
  backendApplied = true;
  versionsWillBeLoaded.set(true);
}
