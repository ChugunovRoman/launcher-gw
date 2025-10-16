<script lang="ts">
  import "normalize.css";
  import Header from "./Components/Header.svelte";
  import MenuBar from "./Components/MenuBar.svelte";
  import LinksBar from "./Components/LinksBar.svelte";
  import LaunchBtn from "./Components/LaunchBtn.svelte";
</script>

<main class="container">
  <!-- svelte-ignore element_invalid_self_closing_tag -->
  <div class="bgimg" />
  <Header />
  <div class="appbody">
    <div class="menubar">
      <MenuBar />
    </div>
    <div class="main" data-tauri-drag-region>
      <LaunchBtn />
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
