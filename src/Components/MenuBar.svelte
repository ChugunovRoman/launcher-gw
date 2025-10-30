<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Home, Settings, RunParams, Pack, Unpack } from "../Icons";
  import { providersWasInited } from "../store/main";

  const { onSelect } = $props<{ onSelect: (view: string) => void }>();

  let activeItem = $state("home");
  let allowPackMod = $state(false);

  $effect(() => {
    if ($providersWasInited) {
      invoke<boolean>("allow_pack_mod").then((value) => (allowPackMod = value));
    }
  });
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
  {#if allowPackMod}
    <div class="baritem" class:active={activeItem === "pack"}>
      <Pack
        size={40}
        onclick={() => {
          onSelect("pack");
          activeItem = "pack";
        }} />
    </div>
    <div class="baritem" class:active={activeItem === "unpack"}>
      <Unpack
        size={40}
        onclick={() => {
          onSelect("unpack");
          activeItem = "unpack";
        }} />
    </div>
  {:else}
    <div></div>
    <div></div>
  {/if}
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
    grid-template-rows: 80px 80px 80px 80px 1fr 80px 40px;
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
