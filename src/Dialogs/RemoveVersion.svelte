<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    expandedKey,
    localVersions,
    removeLocalVersion,
    removeVersion,
    removeVersionInProcess,
    showDlgRemoveVersion,
  } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { mainVersion, selectedVersion } from "../store/upload";
  import { loadVersions } from "../lib/versions";

  function handleClose() {
    console.log("Dlg was closed");
  }

  async function yesHandler() {
    $showDlgRemoveVersion = false;
    $removeVersionInProcess = true;

    try {
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

      // D3: use loadVersions instead of clear()+push() which silently
      // fails to notify Svelte subscribers.  No setTimeout needed.
      try {
        await loadVersions();
      } catch (e) {
        console.error("RemoveVersion: loadVersions failed:", e);
      }

      $expandedKey = null;
    } finally {
      $removeVersionInProcess = false;
    }
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
