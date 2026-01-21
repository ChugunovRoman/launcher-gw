<script lang="ts">
  import { setupGlobalErrorHandlers } from "./errors";
  setupGlobalErrorHandlers();

  import "normalize.css";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";

  import Header from "./Components/Header.svelte";
  import MenuBar from "./Components/MenuBar.svelte";
  import LinksBar from "./Components/LinksBar.svelte";

  import RestartApp from "./Dialogs/RestartApp.svelte";
  import RemoveVersion from "./Dialogs/RemoveVersion.svelte";
  import RemoveProfile from "./Dialogs/RemoveProfile.svelte";
  import CannotApplyProfile from "./Dialogs/CannotApplyProfile.svelte";
  import ApplyProfileOk from "./Dialogs/ApplyProfileOk.svelte";

  import MainView from "./Views/Main.svelte";
  import SettingsView from "./Views/Settings.svelte";
  import PackView from "./Views/Pack.svelte";
  import UnpackView from "./Views/Unpack.svelte";
  import ReleasesView from "./Views/Releases.svelte";
  import RunParamsView from "./Views/RunParams.svelte";
  import KeybindingsView from "./Views/Keybindings.svelte";
  import VersionsView from "./Views/Versions.svelte";
  import TokensView from "./Views/Tokens.svelte";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  import { providersWasInited } from "./store/main";
  import { currentView, previousView } from "./store/menu";

  let bgUrl = "/static/bg.jpg";
  let flyOffset: number = 500;
  const VIEW_ORDER: string[] = ["home", "runParams", "keybindings", "versions", "pack", "unpack", "releases", "tokens", "settings"];

  // Маппинг view -> компонент
  const views: Record<string, any> = {
    home: MainView,
    runParams: RunParamsView,
    keybindings: KeybindingsView,
    versions: VersionsView,
    pack: PackView,
    unpack: UnpackView,
    releases: ReleasesView,
    tokens: TokensView,
    settings: SettingsView,
  };

  $: if ($providersWasInited) {
    loadBackground();
  }

  // Обработчик выбора view
  function handleSelect(view: string) {
    currentView.select(view);
  }
  function getDirection(): "forward" | "backward" {
    if (!$previousView) return "forward";

    const currentIndex = VIEW_ORDER.indexOf($currentView);
    const prevIndex = VIEW_ORDER.indexOf($previousView);

    if (currentIndex > prevIndex) return "forward"; // вперёд → снизу
    if (currentIndex < prevIndex) return "backward"; // назад → сверху
    return "forward";
  }
  function getFlyParams() {
    const dir = getDirection();
    return {
      duration: 500,
      easing: quintOut,
      y: dir === "forward" ? flyOffset : -flyOffset,
      opacity: 0.8,
    };
  }

  async function loadBackground() {
    try {
      const bytes = await invoke<number[]>("get_launcher_bg");
      const blob = new Blob([new Uint8Array(bytes)], { type: "image/jpeg" });
      bgUrl = URL.createObjectURL(blob);
    } catch (err) {
      console.error("Failed to load background:", err);
    }
  }

  onMount(() => {
    flyOffset = Math.round(window.innerHeight);

    const handleResize = () => {
      flyOffset = Math.round(window.innerHeight);
    };
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
    };
  });
</script>

<main class="container">
  <!-- svelte-ignore element_invalid_self_closing_tag -->
  <div class="bgimg" style="background-image: url({bgUrl})" />
  <Header />
  <div class="appbody">
    <div class="menubar">
      <MenuBar onSelect={handleSelect} />
    </div>
    <div class="main" data-tauri-drag-region>
      {#each [$currentView] as view (view)}
        <div in:fly={getFlyParams()} style="width: 100%; height: 100%;">
          <svelte:component this={views[view]} />
        </div>
      {/each}
    </div>
    <div class="bar">
      <LinksBar />
    </div>
  </div>

  <RestartApp />
  <RemoveVersion />
  <RemoveProfile />
  <CannotApplyProfile />
  <ApplyProfileOk />
</main>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 16px;
    line-height: 24px;
    font-weight: 400;

    color: #f6f6f6;
    background-color: #f6f6f6;

    width: 100vw;
    height: 100vh;

    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    -webkit-text-size-adjust: 100%;
    overflow: hidden;
    user-select: none;
  }

  :global(body) {
    height: 100%;
  }
  :global(#app) {
    height: 100%;
  }

  .container {
    margin: 0;
    display: grid;
    grid-template-rows: 42px 1fr;
    justify-content: center;
    text-align: center;
    height: 100%;
  }
  .bgimg {
    position: absolute;
    top: 0px;
    left: 0px;
    width: 100vw;
    height: 100vh;
    background-repeat: no-repeat;
    background-position: center center;
    background-size: cover;
    transition: background-image 0.5s ease;
    z-index: -1;
  }

  .appbody {
    display: grid;
    grid-template-columns: 80px 1fr 80px;
  }
  .main {
    -webkit-app-region: drag;
    position: relative;
    /* overflow: auto;
    height: 95vh; */
  }
  .main::-webkit-scrollbar {
    width: 12px;
  }
  .main::-webkit-scrollbar-track {
    background: transparent;
  }
  .main::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .main::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .main::-webkit-scrollbar-button {
    display: none;
  }
  .bar {
    /* background-color: rgba(0, 0, 0, 0.5); */
  }

  @media (max-width: 1920px) and (max-height: 1080px) {
    .container {
      background-size: 1920px 1080px;
    }
  }
</style>
