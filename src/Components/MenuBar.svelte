<script lang="ts">
  import { Home, Settings, RunParams } from "../Icons";

  export let onSelect: (view: string) => void;

  let activeItem = "home";
</script>

<div class="menubar">
  <div class="baritem" class:active={activeItem === "home"}>
    <Home
      size={40}
      onclick={() => {
        onSelect("home");
        activeItem = "home";
      }} />
  </div>
  <div class="baritem" class:active={activeItem === "runParams"}>
    <RunParams
      size={40}
      onclick={() => {
        onSelect("runParams");
        activeItem = "runParams";
      }} />
  </div>
  <div></div>
  <div class="baritem" class:active={activeItem === "settings"}>
    <Settings
      size={40}
      onclick={() => {
        onSelect("settings");
        activeItem = "settings";
      }} />
  </div>
  <div></div>
</div>

<style>
  .menubar {
    margin-top: 20px;
    display: grid;
    grid-template-rows: 80px 80px 1fr 80px 40px;
    align-items: center;
    background-color: rgba(0, 0, 0, 0);
    justify-items: anchor-center;
    height: 100%;
  }

  .baritem {
    position: relative;
    display: flex;
    align-items: center;
    padding-left: 16px; /* отступ для места под кружок */
  }
  .baritem.active::before {
    content: "";
    position: absolute;
    left: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background-color: white;
  }
</style>
