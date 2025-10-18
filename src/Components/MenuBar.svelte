<script lang="ts">
  import { Home, Settings, RunParams } from "../Icons";

  export let onSelect: (view: string) => void;
</script>

<div class="menubar">
  <Home size={40} onclick={() => onSelect("home")} />
  <RunParams size={40} onclick={() => onSelect("runParams")} />
  <div></div>
  <Settings size={40} onclick={() => onSelect("settings")} />
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
</style>
