<script lang="ts">
  import { _ } from "svelte-i18n";
  import { newLauncherVersionDownloaded, showDlgRestartApp } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";

  function handleClose() {
    console.log("Dlg was closed");
  }
  function yesHandler() {
    $showDlgRestartApp = false;
    invoke("restart_app");
  }
</script>

<Modal bind:showModal={$showDlgRestartApp} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.restartTxt1")} {$newLauncherVersionDownloaded}. <br /> {$_("app.dlg.restartTxt2")}</p>

  {#snippet footer()}
    <Button onclick={yesHandler}>{$_("app.dlg.yes")}</Button>
    <Button isRed onclick={() => ($showDlgRestartApp = false)}>{$_("app.dlg.no")}</Button>
  {/snippet}
</Modal>

<style>
  span,
  p {
    color: white;
  }

  p {
    padding-bottom: 10px;
  }
</style>
