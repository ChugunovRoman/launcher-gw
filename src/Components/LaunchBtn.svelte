<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { localVersions, providersWasInited } from "../store/main";
  import { onMount } from "svelte";
  import { currentView } from "../store/menu";
  import { hasAnyLocalVersion, mainVersion, selectedVersion } from "../store/upload";

  let pid: number | null = $state(null);
  let isProcessAlive = $state(false);
  let interval: number | undefined = undefined;

  const launchApp = async () => {
    if (!$mainVersion && !$selectedVersion) {
      currentView.select("versions");
    }
    if (pid && pid > 0) return;

    try {
      const version = $localVersions.get($selectedVersion!);
      pid = await invoke<number>("run_game", { version: $mainVersion || version });
      await checkProcess();
      interval = setInterval(checkProcess, 1000);
    } catch (err) {
      console.error("Failed to spawn process:", err);
    }
  };

  const checkProcess = async () => {
    if (!pid || pid === -1) return;

    isProcessAlive = await invoke<boolean>("is_process_alive", { pid });

    if (interval && !isProcessAlive) {
      clearInterval(interval);
      pid = null;
    }
  };

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config")
        .then((config) => {
          pid = config.latest_pid;

          if (config.selected_version) {
            selectedVersion.set(config.selected_version);
          }

          if (pid < 0) return;

          return checkProcess();
        })
        .then(() => {
          interval = setInterval(checkProcess, 1000);
        });
    }
  });

  onMount(async () => {
    mainVersion.set(await invoke<Version | undefined>("get_main_version"));
    if ($mainVersion) {
      localVersions.setItem($mainVersion.name, $mainVersion);
      selectedVersion.set($mainVersion.name);
      hasAnyLocalVersion.set(true);
    }
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
