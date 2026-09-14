import { invoke } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import {
  appConfig,
  configReady,
  startupState,
  allowPackMod,
  radioApiProvider,
  fetchLocalVersions,
  providers,
  gameStatus,
  localVersions,
  refreshLocalVersion,
} from '../store/main';
import {
  selectedVersion as selectedVersionStore,
  mainVersion,
  hasAnyLocalVersion,
  showUploading,
  releaseName,
  totalFiles,
  uploadedFiles,
  refreshVersions,
} from '../store/upload';
import { maybeStartUpdateCheck } from './main';
import { applyVersions } from './versions';
import { applyKeyProfiles } from './profiles';

/**
 * Pull initial state from backend commands after mount.
 * Each call is wrapped in its own try/catch so one failure does not block
 * the rest of the UI from appearing.
 */
export async function bootstrap() {
  // 1. get_config → appConfig, radioApiProvider, selectedVersion, upload state
  try {
    const cfg = await invoke<AppConfig>('get_config');
    appConfig.set(cfg);

    if (cfg.selected_provider_id) {
      radioApiProvider.set(cfg.selected_provider_id);
    }
    if (cfg.selected_version) {
      selectedVersionStore.set(cfg.selected_version);
    }

    // Restore an interrupted upload (moved here from LaunchBtn's $effect).
    if (!get(showUploading) && !!cfg.progress_upload && !!cfg.progress_upload.name && !cfg.progress_upload.is_completed) {
      showUploading.set(true);
      releaseName.set(cfg.progress_upload.name);
      totalFiles.set(cfg.progress_upload.total_files);
      uploadedFiles.set(cfg.progress_upload.uploaded_files.length);
    }
  } catch (e) {
    console.error('bootstrap: get_config failed', e);
  }

  // 2. versions from config cache (C7: only if the cached provider matches
  //    the currently selected one, otherwise wait for the backend to emit
  //    versions-loaded with the correct list).
  try {
    const cfg = get(appConfig);
    // CRIT-5: A missing field means the config predates this feature — treat
    // it as a MISMATCH so the backend re-fetches the correct list for the
    // currently selected provider, rather than showing stale data from the
    // last provider that happened to be active.
    const providerMatch = !!cfg.versions_provider_id
      && cfg.versions_provider_id === cfg.selected_provider_id;
    if (cfg.versions && cfg.versions.length > 0 && providerMatch) {
      // Through applyVersions so this write shares the generation counter
      // with loadVersions — the cached list must not overwrite a fresher one.
      await applyVersions(cfg.versions, "cache");
    }
  } catch (e) {
    console.error('bootstrap: versions failed', e);
  }

  // 3. fetchLocalVersions
  try {
    await fetchLocalVersions();
    // Re-emit the map stores so subscribers pick up the freshly added items
    // (same calls LaunchBtn used to make in its $effect).
    refreshLocalVersion();
    refreshVersions();
  } catch (e) {
    console.error('bootstrap: fetchLocalVersions failed', e);
  }

  // 4. get_main_version → mainVersion (moved from LaunchBtn onMount so the
  // launch button works on any view, not just when the main view is mounted).
  try {
    const main = await invoke<Version | undefined>('get_main_version');
    mainVersion.set(main);
    if (main) {
      localVersions.setItem(main.name, main);
      // The store holds ONLY the version next to the launcher; the user's
      // chosen version wins unless nothing is selected yet.
      if (!get(selectedVersionStore)) {
        selectedVersionStore.set(main.name);
      }
      hasAnyLocalVersion.set(true);
    }
  } catch (e) {
    console.error('bootstrap: get_main_version failed', e);
  }

  // 5. allow_pack_mod
  try {
    const allowed = await invoke<boolean>('allow_pack_mod');
    allowPackMod.set(allowed);
  } catch (e) {
    console.error('bootstrap: allow_pack_mod failed', e);
  }

  // 6. get_game_status
  try {
    const status = await invoke<GameStatus>('get_game_status');
    gameStatus.set(status);
  } catch (e) {
    console.error('bootstrap: get_game_status failed', e);
  }

  // 7. provider stats (placeholders until ping finishes — Settings shows
  // "measuring" while startupState.providers is pending)
  try {
    const stats = await invoke<[string, ProviderStatus][]>('get_api_providers_stats');
    stats.sort((a, b) => (a[1].latency_ms ?? Number.MAX_SAFE_INTEGER) - (b[1].latency_ms ?? Number.MAX_SAFE_INTEGER));
    providers.set(stats);
  } catch (e) {
    console.error('bootstrap: providers stats failed', e);
  }

  // 8. get_key_profiles (shared logic with the load-key-profiles event)
  try {
    const keyProfiles = await invoke<ProfileItem[]>('get_key_profiles');
    applyKeyProfiles(keyProfiles);
  } catch (e) {
    console.error('bootstrap: get_key_profiles failed', e);
  }

  // 9. get_startup_state
  try {
    const state = await invoke<StartupState>('get_startup_state');
    startupState.set(state);
    // If the backend already finished its ping before bootstrap ran, the
    // startup-state event was lost — start the update check from here.
    maybeStartUpdateCheck();
  } catch (e) {
    console.error('bootstrap: get_startup_state failed', e);
  }

  // Mark bootstrap complete.
  configReady.set(true);
}
