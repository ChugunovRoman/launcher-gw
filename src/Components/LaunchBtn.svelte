<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { gameStatus, launchError, localVersions, providersWasInited, refreshLocalVersion, showDlgLaunchError } from "../store/main";
  import { onMount } from "svelte";
  import { currentView } from "../store/menu";
  import {
    hasAnyLocalVersion,
    mainVersion,
    refreshVersions,
    releaseName,
    selectedVersion,
    showUploading,
    totalFiles,
    uploadedFiles,
  } from "../store/upload";
  import { normalizeLaunchError } from "../lib/main";

  // Game liveness is owned by the backend GameTracker ($gameStatus mirrors it);
  // the button only sends run_game and shows errors.
  const launchApp = async () => {
    if (!$mainVersion && !$selectedVersion) {
      currentView.select("versions");
      return;
    }
    if ($gameStatus.running) return;

    // Only treat the launch as "main version" when the selection IS the main
    // version — otherwise run the explicitly selected installed version.
    const useMain = !!$mainVersion && $selectedVersion === $mainVersion.name;
    const versionName = useMain ? null : $selectedVersion;
    if (!useMain && !versionName) {
      currentView.select("versions");
      return;
    }
    if (!useMain && versionName && !$localVersions.get(versionName)) {
      console.error("Selected version not found in localVersions:", versionName);
      return;
    }

    try {
      await invoke<GameStatus>("run_game", { versionName, useMain });
    } catch (e) {
      launchError.set(normalizeLaunchError(e));
      showDlgLaunchError.set(true);
    }
  };

  $effect(() => {
    if (!$providersWasInited) return;

    let cancelled = false;

    invoke<AppConfig>("get_config")
      .then(async (config) => {
        if (cancelled) return;

        if (config.selected_version) {
          $selectedVersion = config.selected_version;
        }

        if (!$showUploading && !!config.progress_upload && !!config.progress_upload.name && !config.progress_upload.is_completed) {
          $showUploading = true;
          $releaseName = config.progress_upload.name;
          $totalFiles = config.progress_upload.total_files;
          $uploadedFiles = config.progress_upload.uploaded_files.length;
        }

        refreshLocalVersion();
        refreshVersions();
      })
      .catch((err) => console.error("LaunchBtn get_config failed:", err));

    return () => {
      cancelled = true;
    };
  });

  onMount(async () => {
    mainVersion.set(await invoke<Version | undefined>("get_main_version"));
    if ($mainVersion) {
      localVersions.setItem($mainVersion.name, $mainVersion);
      // The store holds ONLY the version next to the launcher; the user's
      // chosen version wins unless nothing is selected yet.
      if (!$selectedVersion) {
        selectedVersion.set($mainVersion.name);
      }
      hasAnyLocalVersion.set(true);
    }
  });
</script>

<span role="button" tabindex="0" class="launchbtn" class:launchbtn_inactive={$gameStatus.running} onclick={launchApp}>
  {#if !$gameStatus.running}
    {#if $selectedVersion}
      {$_("app.launch.start")} {$selectedVersion}
    {:else}
      {$_("app.launch.download")}
    {/if}
  {:else}
    {$_("app.launch.inGame")}
  {/if}
</span>

<style>
  .launchbtn {
    position: absolute;
    bottom: 50px;
    right: 140px;
    padding: 10px 40px;
    background-color: rgba(61, 93, 236, 0.8);
    transition: background-color 0.15s ease;
    border-radius: 3px;
    -webkit-app-region: no-drag;
  }
  .launchbtn:hover {
    cursor: pointer;
    background-color: rgba(61, 93, 236, 1);
  }
  .launchbtn_inactive {
    cursor: default;
    background-color: rgba(0, 0, 0, 0.8);
  }
  .launchbtn_inactive:hover {
    background-color: rgba(0, 0, 0, 0.8);
  }
</style>
