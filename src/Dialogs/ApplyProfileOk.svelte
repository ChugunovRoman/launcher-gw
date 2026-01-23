<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { selectedProfile, showDlgApplyProfileOk } from "../store/profiles";

  function handleClose() {
    console.log("Dlg was closed");
  }
</script>

<Modal bind:showModal={$showDlgApplyProfileOk} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.applyProfileOk").replace("PROFILE", $selectedProfile!)}</p>

  {#snippet footer()}
    <Button isRed onclick={() => ($showDlgApplyProfileOk = false)}>{$_("app.dlg.ok")}</Button>
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
