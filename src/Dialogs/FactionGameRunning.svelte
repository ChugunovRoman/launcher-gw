<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { showDlgFactionGameRunning } from "../store/factionSettings";

  function close() {
    $showDlgFactionGameRunning = false;
  }
</script>

<Modal bind:showModal={$showDlgFactionGameRunning} onClose={close}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.factionSettings.gameRunning")}</p>

  {#snippet footer()}
    <Button onclick={() => ($showDlgFactionGameRunning = false)}>{$_("app.dlg.ok")}</Button>
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
