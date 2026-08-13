<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { localVersions, providersWasInited, refreshLocalVersion } from "../store/main";
  import { onDestroy, onMount } from "svelte";
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

  let pid: number | null = $state(null);
  let isProcessAlive = $state(false);
  let interval: number | undefined = undefined;

  const clearPidInterval = () => {
    if (interval !== undefined) {
      clearInterval(interval);
      interval = undefined;
    }
  };

  const launchApp = async () => {
    if (!$mainVersion && !$selectedVersion) {
      currentView.select("versions");
      return;
    }
    if (pid && pid > 0 && (await checkProcess())) return;

    const useMain = !!$mainVersion;
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
      clearPidInterval();
      pid = await invoke<number>("run_game", { versionName, useMain });
      await checkProcess();
      interval = setInterval(checkProcess, 1000);
    } catch (err) {
      console.error("Failed to spawn process:", err);
    }
  };

  const checkProcess = async () => {
    if (!pid || pid === -1) return false;

    isProcessAlive = await invoke<boolean>("is_process_alive", { pid });

    if (interval !== undefined && !isProcessAlive) {
      clearPidInterval();
      pid = null;
    }

    return isProcessAlive;
  };

  $effect(() => {
    if (!$providersWasInited) return;

    let cancelled = false;

    invoke<AppConfig>("get_config")
      .then(async (config) => {
        if (cancelled) return;

        pid = config.latest_pid;

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

        if (pid != null && pid >= 0) {
          await checkProcess();
        }

        if (!cancelled) {
          clearPidInterval();
          interval = setInterval(checkProcess, 1000);
        }
      })
      .catch((err) => console.error("LaunchBtn get_config failed:", err));

    return () => {
      cancelled = true;
      clearPidInterval();
    };
  });

  onMount(async () => {
    mainVersion.set(await invoke<Version | undefined>("get_main_version"));
    if ($mainVersion) {
      localVersions.setItem($mainVersion.name, $mainVersion);
      selectedVersion.set($mainVersion.name);
      hasAnyLocalVersion.set(true);
    }
  });

  onDestroy(() => {
    clearPidInterval();
  });
</script>

<span role="button" tabindex="0" class="launchbtn" class:launchbtn_inactive={isProcessAlive} onclick={launchApp}>
  {#if !isProcessAlive}
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
