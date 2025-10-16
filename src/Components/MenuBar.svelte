<script lang="ts">
  import { open } from "@tauri-apps/plugin-shell";

  import { Home } from "../Icons";
</script>

<div class="linksbar">
  <Home size={40} onclick={() => ""} />
</div>

<style>
  .linksbar {
    margin-top: 20px;
    display: grid;
    grid-template-rows: 80px 80px 80px 80px 80px 80px 80px 1fr;
    align-items: center;
    background-color: rgba(0, 0, 0, 0);
    justify-items: anchor-center;
  }
</style>
