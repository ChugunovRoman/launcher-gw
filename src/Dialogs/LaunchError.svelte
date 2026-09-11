<script lang="ts">
  import { _ } from "svelte-i18n";
  import { launchError, showDlgLaunchError } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";

  function handleClose() {
    console.log("Dlg was closed");
  }
  function okHandler() {
    $showDlgLaunchError = false;
  }
</script>

<Modal bind:showModal={$showDlgLaunchError} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.launchErrorTitle")}</span>
  {/snippet}

  {#if $launchError}
    <p>{$_(`app.launchError.${$launchError.code}`)}</p>
    {#if $launchError.detail}
      <p class="error-detail">{$launchError.detail}</p>
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

  .error-detail {
    font-family: monospace;
    font-size: 0.85rem;
    color: #f55858;
    word-break: break-word;
  }
</style>
