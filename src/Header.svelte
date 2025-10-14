<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  import { X, Minus, Minimize2, Maximize2, Settings } from "lucide-svelte";

  let isMaximized = $state(false);
  let savedSize: { width: number; height: number } | null = null;

  const closeWindowHandler = async () => {
    console.log("Close");
    await invoke("app_exit");
  };
  const minimizeWindowHandler = () => console.log("Minimize");
  const toggleMaximizeHandler = async () => {
    const [x, y] = await invoke("get_window_size");
    console.log("Toggle maximize, x, y: ", x, y);
  };
  const openSettingsHandler = () => console.log("Open settings");
</script>

<header data-tauri-drag-region>
  <h5 class="titile" data-tauri-drag-region>Global War Launcher</h5>

  <div class="btn"><Settings onclick={openSettingsHandler} size={16} /></div>
  <div class="btn">
    {#if isMaximized}
      <Minimize2 onclick={toggleMaximizeHandler} size={16} />
    {:else}
      <Maximize2 onclick={toggleMaximizeHandler} size={16} />
    {/if}
  </div>
  <div class="btn"><Minus onclick={minimizeWindowHandler} size={16} /></div>
  <div class="btn close"><X onclick={closeWindowHandler} size={16} /></div>
</header>

<style>
  header {
    display: grid;
    grid-template-columns: 1fr 32px 32px 32px 32px 32px;
    align-items: center;
    background-color: rgba(0, 0, 0, 0.5);
    width: 100vw;
  }

  .titile {
    text-align: left;
    text-indent: 3%;
    padding: 0;
    margin: 2px 0;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    cursor: pointer;
    border-radius: 4px;
    color: var(--text-color, white);
    transition:
      color 0.15s ease,
      background-color 0.15s ease;
    outline: none;
  }

  .btn:hover {
    color: rgba(197, 228, 254, 0.9);
  }
  .close:hover {
    color: rgba(254, 197, 208, 0.9);
  }
</style>
