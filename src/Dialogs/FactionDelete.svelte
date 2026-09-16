<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { showDlgFactionDelete, factionDeleteTarget, factionBusy, factionSelectedId } from "../store/factionSettings";
  import { deleteFactionProfile, loadFactionProfiles, factionErrorKey, factionMessageDetail } from "../lib/factionSettings";

  let error = $state("");

  function close() {
    $showDlgFactionDelete = false;
    $factionDeleteTarget = undefined;
    error = "";
  }

  async function yesHandler() {
    if ($factionBusy) return;
    const target = $factionDeleteTarget;
    if (!target) return;
    error = "";

    $factionBusy = true;
    try {
      await deleteFactionProfile(target.id);
      if ($factionSelectedId === target.id) {
        $factionSelectedId = undefined;
      }
      await loadFactionProfiles();
      close();
    } catch (err) {
      console.error("faction profile delete failed:", err);
      error = `${$_(factionErrorKey(err))} ${factionMessageDetail(String(err))}`.trim();
    } finally {
      $factionBusy = false;
    }
  }
</script>

<Modal bind:showModal={$showDlgFactionDelete} onClose={close}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.factionSettings.confirmDelete")} "{$factionDeleteTarget?.name}"?</p>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#snippet footer()}
    <Button isRed isDisabled={$factionBusy} onclick={yesHandler}>{$_("app.dlg.yes")}</Button>
    <Button isDisabled={$factionBusy} onclick={close}>{$_("app.dlg.no")}</Button>
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
  .error {
    color: #f55858;
  }
</style>
