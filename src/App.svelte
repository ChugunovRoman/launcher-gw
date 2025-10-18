<script lang="ts">
  import "normalize.css";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";

  import Header from "./Components/Header.svelte";
  import MenuBar from "./Components/MenuBar.svelte";
  import LinksBar from "./Components/LinksBar.svelte";

  import MainView from "./Views/Main.svelte";
  import SettingsView from "./Views/Settings.svelte";
  import RunParamsView from "./Views/RunParams.svelte";
  import { onMount } from "svelte";

  let flyOffset: number = 500;
  const VIEW_ORDER: string[] = ["home", "runParams", "settings"];
  let currentView = "home";
  let previousView: string | null = null;

  // Маппинг view -> компонент
  const views: Record<string, any> = {
    home: MainView,
    runParams: RunParamsView,
    settings: SettingsView,
  };

  // Обработчик выбора view
  function handleSelect(view: string) {
    if (view !== currentView) {
      previousView = currentView;
      currentView = view;
    }
  }
  function getDirection(): "forward" | "backward" {
    if (!previousView) return "forward";

    const currentIndex = VIEW_ORDER.indexOf(currentView);
    const prevIndex = VIEW_ORDER.indexOf(previousView);

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
  <div class="bgimg" />
  <Header />
  <div class="appbody">
    <div class="menubar">
      <MenuBar onSelect={handleSelect} />
    </div>
    <div class="main" data-tauri-drag-region>
      {#each [currentView] as view (view)}
        <div in:fly={getFlyParams()} style="width: 100%; height: 100%;">
          <svelte:component this={views[view]} />
        </div>
      {/each}
    </div>
    <div class="bar">
      <LinksBar />
    </div>
  </div>
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
    background-image: url("/static/bg.jpg");
    background-repeat: no-repeat;
    background-position: center center;
    background-size: cover;
    z-index: -1;
  }

  .appbody {
    display: grid;
    grid-template-columns: 80px 1fr 80px;
  }
  .main {
    -webkit-app-region: drag;
    position: relative;
    height: 100%;
    overflow: hidden;
  }
  .bar {
    background-color: rgba(0, 0, 0, 0.5);
  }

  @media (max-width: 1920px) and (max-height: 1080px) {
    .container {
      background-size: 1920px 1080px;
    }
  }
</style>
