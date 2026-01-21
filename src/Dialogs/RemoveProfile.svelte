<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./index.svelte";
  import Button from "../Components/Button.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { profileKeyMap, profiles, removeProfileName, showDlgRemoveProfile, updateCurrentBindsMap } from "../store/profiles";

  function handleClose() {
    console.log("Dlg was closed");
  }
  async function yesHandler() {
    await invoke<void>("delete_profile", { name: $removeProfileName });

    profileKeyMap.delItem($removeProfileName!);
    profiles.set($profiles.filter((p) => p.value !== $removeProfileName));

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
