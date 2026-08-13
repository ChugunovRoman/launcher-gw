<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { profileKeyMap, profiles, removeProfileName, selectedProfile, showDlgRemoveProfile, updateCurrentBindsMap, applyKeyProfile } from "../store/profiles";
  import { persistProfileSelection } from "../lib/profiles";
  import { DEFAULT_BIND_LTX } from "../consts";

  function handleClose() {
    console.log("Dlg was closed");
  }
  async function yesHandler() {
    const removed = $removeProfileName!;
    await invoke<void>("delete_profile", { name: removed });

    profileKeyMap.delItem(removed);
    profiles.set($profiles.filter((p) => p.value !== removed));

    if ($selectedProfile === removed) {
      selectedProfile.set(DEFAULT_BIND_LTX);
      await persistProfileSelection(DEFAULT_BIND_LTX, $applyKeyProfile);
    }

    updateCurrentBindsMap();

    $showDlgRemoveProfile = false;
    $removeProfileName = undefined;
  }
</script>

<Modal bind:showModal={$showDlgRemoveProfile} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.removeProfile")} "{$removeProfileName?.replace(".ltx", "")}"?</p>

  {#snippet footer()}
    <Button onclick={yesHandler}>{$_("app.dlg.yes")}</Button>
    <Button isRed onclick={() => ($showDlgRemoveProfile = false)}>{$_("app.dlg.no")}</Button>
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
