<script lang="ts">
  import { _ } from "svelte-i18n";
  import { expandedIndex, localVersions, removeLocalVersion, removeVersion, removeVersionInProcess, showDlgRemoveVersion } from "../store/main";
  import Modal from "./index.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { mainVersion, refreshVersions, selectedVersion } from "../store/upload";

  function handleClose() {
    console.log("Dlg was closed");
  }
  async function yesHandler() {
    $showDlgRemoveVersion = false;
    $removeVersionInProcess = true;

    const version = $removeVersion!;
    await invoke<void>("delete_installed_version", { versionName: version.path });

    removeLocalVersion(version.name);

    if ($localVersions.size) {
      const name = [...$localVersions.keys()][0];
      await invoke<void>("set_current_game_version", { versionName: name });
      selectedVersion.set(name);
    } else if ($mainVersion) {
      await invoke<void>("set_current_game_version", { versionName: $mainVersion.name });
      selectedVersion.set($mainVersion.name);
    } else {
      await invoke<void>("set_current_game_version");
      selectedVersion.set(undefined);
    }

    $removeVersionInProcess = false;

    refreshVersions();

    $expandedIndex = null;
  }
</script>

<Modal bind:showModal={$showDlgRemoveVersion} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.removeVersion")} {$removeVersion?.name}?</p>

  {#snippet footer()}
    <Button onclick={yesHandler}>{$_("app.dlg.yes")}</Button>
    <Button isRed onclick={() => ($showDlgRemoveVersion = false)}>{$_("app.dlg.no")}</Button>
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
