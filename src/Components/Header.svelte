<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { saveWindowState, StateFlags } from "@tauri-apps/plugin-window-state";
  import { _ } from "svelte-i18n";
  import { locale } from "svelte-i18n";

  import { X, Minus, Minimize2, Maximize2 } from "lucide-svelte";
  import { Lang } from "../consts";
  import { connectStatus, fontColor } from "../store/main";

  let isMaximized = $state(false);
  let version = $state("0.1.0");

  let langs = $state([Lang.Ru, Lang.En]);
  let currentLangIndex = $state(0);
  let langIconPath = $derived(`/static/lang/${langs[currentLangIndex]}.png`);

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
  const toggleLang = async () => {
    currentLangIndex++;
    if (currentLangIndex >= langs.length) currentLangIndex = 0;

    const lang = langs[currentLangIndex];
    $locale = lang;

    await invoke<string>("set_lang", { lang });
  };

  async function loadLang() {
    const lang = await invoke<Lang>("get_lang");
    currentLangIndex = langs.indexOf(lang);
    $locale = lang;
    console.log("loadLang: ", lang, currentLangIndex);
  }

  onMount(async () => {
    version = await getVersion();
    console.log("version: ", version);
    loadLang();
  });
</script>

<header>
  <h5 class="title" role="button" tabindex="0" ondblclick={toggleMaximizeHandler}>
    Global War Launcher {version} <span style="color: {$fontColor}; font-size: 0.7rem">{$_(`app.h.${$connectStatus}`)}</span>
  </h5>

  <div role="button" onclick={toggleLang} class="btn">
    <img class="langicon" src={langIconPath} alt={langs[currentLangIndex]} />
  </div>
  <div></div>
  <div role="button" onclick={toggleMaximizeHandler} class="btn">
    {#if isMaximized}
      <Minimize2 size={16} />
    {:else}
      <Maximize2 size={16} />
    {/if}
  </div>
  <div role="button" onclick={minimizeWindowHandler} class="btn"><Minus size={16} /></div>
  <div role="button" onclick={closeWindowHandler} class="btn close"><X size={16} /></div>
</header>

<style>
  header {
    display: grid;
    grid-template-columns: 1fr 48px 10px 48px 48px 48px;
    align-items: center;
    background-color: rgba(51, 219, 79, 0);
    width: 100vw;
  }
  h5 {
    -webkit-app-region: drag;
  }

  .title {
    text-align: left;
    text-indent: 3%;
    padding: 0;
    margin: 4px 0;
  }
  .langicon {
    width: 18px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 4px;
    color: var(--text-color, white);
    transition:
      color 0.15s ease,
      background-color 0.15s ease;
    outline: none;
    -webkit-app-region: no-drag;
  }

  .btn:hover {
    color: rgba(197, 228, 254, 0.9);
    cursor: pointer;
  }
  .close:hover {
    color: rgba(254, 197, 208, 0.9);
  }
</style>
