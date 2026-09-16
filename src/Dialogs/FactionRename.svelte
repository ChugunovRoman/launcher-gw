<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import Bg from "../Components/Bg.svelte";
  import { showDlgFactionRename, factionRenameTarget, factionBusy } from "../store/factionSettings";
  import { updateFactionProfileMeta, loadFactionProfiles, factionErrorKey, factionMessageDetail } from "../lib/factionSettings";

  let name = $state("");
  let description = $state("");
  let error = $state("");

  $effect(() => {
    if ($factionRenameTarget) {
      name = $factionRenameTarget.name;
      description = $factionRenameTarget.description;
      error = "";
    }
  });

  function close() {
    $showDlgFactionRename = false;
    $factionRenameTarget = undefined;
  }

  async function saveHandler() {
    if ($factionBusy) return;
    const target = $factionRenameTarget;
    if (!target || !name.trim()) return;
    error = "";

    $factionBusy = true;
    try {
      await updateFactionProfileMeta(target.id, name, description);
      await loadFactionProfiles();
      close();
    } catch (err) {
      console.error("faction profile rename failed:", err);
      error = `${$_(factionErrorKey(err))} ${factionMessageDetail(String(err))}`.trim();
    } finally {
      $factionBusy = false;
    }
  }
</script>

<Modal bind:showModal={$showDlgFactionRename} onClose={close}>
  {#snippet header()}
    <span>{$_("app.factionSettings.editMeta")}</span>
  {/snippet}

  <Bg>
    <span class="label">{$_("app.factionSettings.name")}</span>
    <input type="text" bind:value={name} maxlength="64" class="text-input" />
  </Bg>
  <Bg>
    <span class="label">{$_("app.factionSettings.description")}</span>
    <textarea bind:value={description} maxlength="1024" rows="3" class="text-input"></textarea>
  </Bg>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#snippet footer()}
    <Button isDisabled={$factionBusy || !name.trim()} onclick={saveHandler}>{$_("app.factionSettings.save")}</Button>
    <Button isRed isDisabled={$factionBusy} onclick={close}>{$_("app.dlg.close")}</Button>
  {/snippet}
</Modal>

<style>
  span {
    color: white;
  }
  .label {
    display: block;
    margin-bottom: 4px;
  }
  .error {
    color: #f55858;
    margin: 0 0 6px 0;
  }
  .text-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
    font-family: inherit;
    resize: vertical;
  }
  .text-input:focus {
    background-color: rgba(255, 255, 255, 1);
  }
</style>
