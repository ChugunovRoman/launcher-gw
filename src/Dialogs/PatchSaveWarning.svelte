<script lang="ts">
  import { _ } from "svelte-i18n";
  import { patchSaveWarningContext, patchSaveWarningProceed, showDlgPatchSaveWarning } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";

  // No "don't show again": the warning is rare and the damage it prevents is
  // irreversible, so it must fire on every install of a save-breaking patch.
  // The context is read BEFORE closing: closing triggers onClose, which clears it.
  function continueHandler() {
    const ctx = $patchSaveWarningContext;
    $showDlgPatchSaveWarning = false;
    if (ctx) patchSaveWarningProceed.set(ctx);
  }

  // Fires on every close, including the programmatic one from the buttons:
  // <dialog>.close() dispatches "close", which Base.svelte routes here.
  // Dropping the context is enough — "continue" already captured what it needs.
  function handleClose() {
    patchSaveWarningContext.set(null);
  }
</script>

<Modal bind:showModal={$showDlgPatchSaveWarning} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.patches.saveBreakWarning")}</p>
  <p class="patch-name">{$patchSaveWarningContext?.patchName}</p>

  {#snippet footer()}
    <Button onclick={continueHandler}>{$_("app.patches.saveBreakContinue")}</Button>
    <Button isRed onclick={() => ($showDlgPatchSaveWarning = false)}>{$_("app.dlg.no")}</Button>
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

  .patch-name {
    font-weight: bold;
  }
</style>
