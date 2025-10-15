<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { saveWindowState, StateFlags } from '@tauri-apps/plugin-window-state';

  import { X, Minus, Minimize2, Maximize2, Settings } from "lucide-svelte";

  let isMaximized = $state(false);
  let version = $state("0.1.0");

  const window = getCurrentWindow();

  const windowMoveHandler = async () => {
    isMaximized = await window.isMaximized();
  };

  window.onMoved(windowMoveHandler); 
  window.onResized(windowMoveHandler); 

  const closeWindowHandler = async () => {
    await invoke("app_exit");
  };
  const minimizeWindowHandler = () => window.minimize();
  const toggleMaximizeHandler = async () => {
    await window.toggleMaximize();

    isMaximized = await window.isMaximized();

    await saveWindowState(StateFlags.ALL);
  };
  const openSettingsHandler = () => console.log("Open settings");

  onMount(async () => {
    version = await getVersion();
    console.log('version: ', version);
  });
</script>

<header data-tauri-drag-region role="button" tabindex="0" ondblclick={toggleMaximizeHandler}>
  <h5 class="titile" data-tauri-drag-region>Global War Launcher {version}</h5>

  <!-- <div class="btn"><Settings onclick={openSettingsHandler} size={16} /></div> -->
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
    grid-template-columns: 1fr 48px 48px 48px;
    align-items: center;
    background-color: rgba(0, 0, 0, 0.0);
    width: 100vw;

    -webkit-app-region: drag;
  }
  h5 {
    -webkit-app-region: drag;
  }

  .titile {
    text-align: left;
    text-indent: 3%;
    padding: 0;
    margin: 4px 0;
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
