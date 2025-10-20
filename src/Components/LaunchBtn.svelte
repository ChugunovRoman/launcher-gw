<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let pid: number | null = $state(null);
  let isProcessAlive = $state(false);
  let interval: number | undefined = undefined;

  const launchApp = async () => {
    if (pid && pid > 0) return;

    try {
      pid = await invoke<number>("run_game");
      await checkProcess();
      interval = setInterval(checkProcess, 1000);
    } catch (err) {
      console.error("Failed to spawn process:", err);
    }
  };

  const checkProcess = async () => {
    if (!pid) return;

    isProcessAlive = await invoke<boolean>("is_process_alive", { pid });

    if (interval && !isProcessAlive) {
      clearInterval(interval);
      pid = null;
    }
  };

  onMount(async () => {
    const config = await invoke<AppConfig>("get_config");
    pid = config.latest_pid;

    if (pid < 0) return;

    await checkProcess();
    interval = setInterval(checkProcess, 1000);
  });
</script>

<span role="button" tabindex="0" class="launchbtn" class:launchbtn_inactive={isProcessAlive} onclick={launchApp}>
  {#if !isProcessAlive}
    {$_("app.launch.start")}
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
