<script lang="ts">
  import { Home, Settings, RunParams, Pack, Unpack, Releases, Versions, Tokens } from "../Icons";
  import { allowPackMod } from "../store/main";
  import { currentView } from "../store/menu";

  const { onSelect } = $props<{ onSelect: (view: string) => void }>();
</script>

<div class="menubar">
  <div class="baritem" class:active={$currentView === "home"}>
    <Home
      size={40}
      onclick={() => {
        onSelect("home");
      }} />
  </div>
  <div class="baritem" class:active={$currentView === "runParams"}>
    <RunParams
      size={40}
      onclick={() => {
        onSelect("runParams");
      }} />
  </div>
  <div class="baritem" class:active={$currentView === "versions"}>
    <Versions
      size={54}
      onclick={() => {
        onSelect("versions");
      }} />
  </div>
  {#if $allowPackMod}
    <div class="baritem" class:active={$currentView === "pack"}>
      <Pack
        size={40}
        onclick={() => {
          onSelect("pack");
        }} />
    </div>
    <div class="baritem" class:active={$currentView === "unpack"}>
      <Unpack
        size={40}
        onclick={() => {
          onSelect("unpack");
        }} />
    </div>
    <div class="baritem" class:active={$currentView === "releases"}>
      <Releases
        size={40}
        onclick={() => {
          onSelect("releases");
        }} />
    </div>
    <div class="baritem" class:active={$currentView === "tokens"}>
      <Tokens
        size={40}
        onclick={() => {
          onSelect("tokens");
        }} />
    </div>
  {:else}
    <div></div>
    <div></div>
    <div></div>
    <div></div>
  {/if}
  <div></div>
  <div class="baritem" class:active={$currentView === "settings"}>
    <Settings
      size={40}
      onclick={() => {
        onSelect("settings");
      }} />
  </div>
  <div></div>
</div>

<style>
  .menubar {
    margin-top: 20px;
    display: grid;
    grid-template-rows: 80px 80px 80px 80px 80px 80px 80px 1fr 80px 40px;
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
