<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./index.svelte";
  import Button from "../Components/Button.svelte";
  import { showDlgApplyProfile } from "../store/profiles";

  function handleClose() {
    console.log("Dlg was closed");
  }
</script>

<Modal bind:showModal={$showDlgApplyProfile} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.cannotApplyProfile")}</p>

  {#snippet footer()}
    <Button isRed onclick={() => ($showDlgApplyProfile = false)}>{$_("app.dlg.ok")}</Button>
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
