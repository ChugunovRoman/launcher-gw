<script lang="ts">
  import { _ } from "svelte-i18n";
  import { showDlgTempPathWarning, tempPathWarningCodes, tempPathWarningKind } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";

  function handleClose() {
    console.log("Dlg was closed");
  }
  function okHandler() {
    $showDlgTempPathWarning = false;
  }
</script>

<Modal bind:showModal={$showDlgTempPathWarning} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  {#if $tempPathWarningKind === "launcher"}
    <p>{$_("app.dlg.launcherInTempDir")}</p>
  {:else}
    {#if $tempPathWarningCodes.includes("temp_dir")}
      <p>{$_("app.dlg.tempPathWarning")}</p>
    {/if}
    {#if $tempPathWarningCodes.includes("rar_temp")}
      <p>{$_("app.dlg.rarTempWarning")}</p>
    {/if}
  {/if}

  {#snippet footer()}
    <Button onclick={okHandler}>{$_("app.dlg.ok")}</Button>
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
