<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    appConfig,
    expandedIndex,
    localVersions,
    removeLocalVersion,
    removeVersion,
    removeVersionInProcess,
    showDlgRemoveVersion,
  } from "../store/main";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { mainVersion, selectedVersion, versions } from "../store/upload";
  import { sep } from "@tauri-apps/api/path";
  import { prepareVersionItem } from "../lib/main";

  function handleClose() {
    console.log("Dlg was closed");
  }
  function hasLocalVersion(version: Version) {
    for (const [name, local] of $localVersions) {
      if (name === version.name) return true;
      if (local.path === version.name) return true;
      if (local.path === version.path) return true;
    }

    return false;
  }
  async function yesHandler() {
    $showDlgRemoveVersion = false;
    $removeVersionInProcess = true;

    const version = $removeVersion!;
    const separ = await sep();
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

    setTimeout(() => {
      invoke<Version[]>("get_available_versions").then((data) => {
        versions.clear();

        for (const item of data) {
          const found = $versions.find((v) => v.name === item.name);
          const hasLocal = hasLocalVersion(item);
          if (!found && !hasLocal) {
            $versions.push(prepareVersionItem($appConfig, item, separ));
          }
        }
      });
    }, 200);

    $removeVersionInProcess = false;

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
