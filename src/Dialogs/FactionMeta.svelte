<script lang="ts">
  import { _ } from "svelte-i18n";
  import { get } from "svelte/store";
  import { save } from "@tauri-apps/plugin-dialog";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import Bg from "../Components/Bg.svelte";
  import { appConfig, updateConfig } from "../store/main";
  import { showDlgFactionMeta, factionMetaMode, factionBusy, factionResult, showDlgFactionResult } from "../store/factionSettings";
  import {
    saveCurrentFactionProfile,
    exportFactionBundle,
    loadFactionProfiles,
    factionErrorKey,
    factionMessageDetail,
    factionBundleFileName,
  } from "../lib/factionSettings";

  let name = $state("");
  let description = $state("");
  let author = $state("");
  let error = $state("");

  // Reset the form on every open. `get(appConfig)` (not `$appConfig`) keeps
  // this effect off the config store: a background config refresh while the
  // dialog is open must not wipe what the player has typed.
  $effect(() => {
    if ($showDlgFactionMeta) {
      name = "";
      description = "";
      author = get(appConfig)?.faction_bundle_author ?? "";
      error = "";
    }
  });

  function close() {
    $showDlgFactionMeta = false;
  }

  async function confirm() {
    if ($factionBusy || !name.trim()) return;
    error = "";

    let destPath: string | null = null;
    if ($factionMetaMode === "exportCurrent") {
      destPath = await save({
        filters: [{ name: "Faction editor settings", extensions: ["gwfe"] }],
        defaultPath: factionBundleFileName(name),
      });
      if (!destPath) return;
    }

    $factionBusy = true;
    try {
      if ($factionMetaMode === "save") {
        await saveCurrentFactionProfile(name, description, author);
        await loadFactionProfiles();
      } else {
        await exportFactionBundle(destPath!, name, description, author);
        $factionResult = { ok: true, message: $_("app.factionSettings.exportSaved"), detail: destPath!, warnings: [], backupPath: null };
        $showDlgFactionResult = true;
      }
      // The backend persisted the author; mirror it so the next open prefills it.
      updateConfig("faction_bundle_author", author.trim() || null);
      close();
    } catch (err) {
      console.error("faction meta save/export failed:", err);
      error = `${$_(factionErrorKey(err))} ${factionMessageDetail(String(err))}`.trim();
    } finally {
      $factionBusy = false;
    }
  }
</script>

<Modal bind:showModal={$showDlgFactionMeta} onClose={close}>
  {#snippet header()}
    <span>{$factionMetaMode === "save" ? $_("app.factionSettings.saveAsTitle") : $_("app.factionSettings.exportCurrentTitle")}</span>
  {/snippet}

  <Bg>
    <span class="label">{$_("app.factionSettings.name")}</span>
    <input type="text" bind:value={name} maxlength="64" class="text-input" placeholder={$_("app.factionSettings.name")} />
  </Bg>
  <Bg>
    <span class="label">{$_("app.factionSettings.description")}</span>
    <textarea bind:value={description} maxlength="1024" rows="3" class="text-input"></textarea>
  </Bg>
  <Bg>
    <span class="label">{$_("app.factionSettings.author")}</span>
    <input type="text" bind:value={author} maxlength="64" class="text-input" placeholder={$_("app.factionSettings.author")} />
  </Bg>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#snippet footer()}
    <Button isDisabled={$factionBusy || !name.trim()} onclick={confirm}>
      {$factionMetaMode === "save" ? $_("app.factionSettings.save") : $_("app.btn.export")}
    </Button>
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
