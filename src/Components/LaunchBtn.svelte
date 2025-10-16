<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let pid = $state(null);
  let isProcessAlive = $state(false);
  let interval: number | undefined = undefined;

  const launchApp = async () => {
    if (pid) return;

    try {
      pid = await invoke("run_game");
      await checkProcess();
      interval = setInterval(checkProcess, 1000);
    } catch (err) {
      console.error("Failed to spawn process:", err);
    }
  };

  const checkProcess = async () => {
    if (!pid) return;

    isProcessAlive = await invoke("is_process_alive", { pid });

    if (interval && !isProcessAlive) {
      clearInterval(interval);
      pid = null;
    }
  };
</script>

<span role="button" tabindex="0" class="launchbtn" class:launchbtn_inactive={isProcessAlive} onclick={launchApp}>
  {#if !isProcessAlive}
    Start Game
  {:else}
    In the Game...
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
  }
  .launchbtn:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .launchbtn_inactive {
    background-color: rgba(0, 0, 0, 0.8);
  }
  .launchbtn_inactive:hover {
    background-color: rgba(0, 0, 0, 0.8);
  }
</style>
