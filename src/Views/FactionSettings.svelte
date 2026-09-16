<script lang="ts">
  import { _ } from "svelte-i18n";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { Upload, Download, Save, RotateCcw, Pencil, Trash2 } from "lucide-svelte";

  import Scroll from "../Components/Scroll.svelte";
  import Bg from "../Components/Bg.svelte";
  import Button from "../Components/Button.svelte";
  import FactionSummary from "../Components/FactionSummary.svelte";

  import { localVersions, appConfig } from "../store/main";
  import { selectedVersion as launchVersion } from "../store/upload";
  import { FACTION_DEFAULT_VERSION_NAME } from "../consts";
  import {
    factionProfiles,
    factionVersionName,
    factionSelectedId,
    factionBusy,
    factionOpError,
    showDlgFactionMeta,
    factionMetaMode,
    showDlgFactionRename,
    factionRenameTarget,
    showDlgFactionDelete,
    factionDeleteTarget,
    showDlgFactionApplyConfirm,
    factionApplyContext,
    showDlgFactionGameRunning,
    factionResult,
    showDlgFactionResult,
  } from "../store/factionSettings";
  import {
    loadFactionProfiles,
    isFactionGameRunning,
    inspectFactionBundle,
    inspectFactionProfile,
    importFactionProfile,
    exportFactionProfile,
    factionErrorKey,
    factionMessageDetail,
    factionBundleFileName,
    setFactionVersion,
  } from "../lib/factionSettings";

  let selected = $derived($factionProfiles.find((p) => p.id === $factionSelectedId));
  let versionOptions = $derived([...$localVersions.values()].map((v) => v.name).sort((a, b) => a.localeCompare(b)));

  onMount(async () => {
    try {
      await loadFactionProfiles();
    } catch (e) {
      showError(e);
    }
  });

  // The version list arrives asynchronously (bootstrap fetches it), so the
  // preselection runs whenever it changes rather than once on mount. The
  // current pick is read with `get` so writing it back does not re-trigger
  // this effect.
  $effect(() => {
    const names = [...$localVersions.values()].map((v) => v.name);
    const saved = $appConfig?.faction_settings_version;
    if (names.length === 0 || get(factionVersionName)) return;

    const dev = names.find((n) => n.toLowerCase() === FACTION_DEFAULT_VERSION_NAME.toLowerCase());
    // Preference order: the version saved for this screen, the dev build, the
    // version selected for launching, then whatever is installed.
    const resolved = (saved && names.find((n) => n === saved)) || dev || names.find((n) => n === get(launchVersion)) || names[0];

    factionVersionName.set(resolved);
  });

  async function handleVersionChange(name: string) {
    $factionOpError = "";
    $factionVersionName = name;
    $factionSelectedId = undefined;
    try {
      await setFactionVersion(name);
    } catch (e) {
      showError(e);
    }
  }

  function showError(e: unknown) {
    console.error("faction settings:", e);
    $factionOpError = `${$_(factionErrorKey(e))} ${factionMessageDetail(String(e))}`.trim();
  }

  function selectProfile(id: string) {
    $factionSelectedId = id;
  }

  /** `true` when it is safe to proceed; otherwise the "close the game first"
   * dialog is shown. */
  async function guardGameRunning(): Promise<boolean> {
    if (await isFactionGameRunning()) {
      $showDlgFactionGameRunning = true;
      return false;
    }
    return true;
  }

  function openSaveAsDialog() {
    $factionOpError = "";
    $factionMetaMode = "save";
    $showDlgFactionMeta = true;
  }

  function openExportCurrentDialog() {
    $factionOpError = "";
    $factionMetaMode = "exportCurrent";
    $showDlgFactionMeta = true;
  }

  async function handleResetDefault() {
    if ($factionBusy) return;
    $factionOpError = "";
    $factionBusy = true;
    try {
      if (!(await guardGameRunning())) return;
      $factionApplyContext = { kind: "resetDefault" };
      $showDlgFactionApplyConfirm = true;
    } catch (e) {
      showError(e);
    } finally {
      $factionBusy = false;
    }
  }

  async function handleApplyProfile(id: string) {
    if ($factionBusy) return;
    $factionOpError = "";
    $factionBusy = true;
    try {
      if (!(await guardGameRunning())) return;
      // Re-inspect against the local install so the confirm dialog can warn
      // about factions this mod version does not know.
      const inspected = await inspectFactionProfile(id);
      $factionApplyContext = { kind: "applyProfile", id, manifest: inspected.manifest, warnings: inspected.warnings };
      $showDlgFactionApplyConfirm = true;
    } catch (e) {
      showError(e);
    } finally {
      $factionBusy = false;
    }
  }

  async function handleImportFile() {
    if ($factionBusy) return;
    $factionOpError = "";
    const path = await open({ multiple: false, filters: [{ name: "Faction editor settings", extensions: ["gwfe"] }] });
    if (!path || Array.isArray(path)) return;

    $factionBusy = true;
    try {
      if (!(await guardGameRunning())) return;
      const inspected = await inspectFactionBundle(path);
      $factionApplyContext = { kind: "importFile", path, manifest: inspected.manifest, warnings: inspected.warnings };
      $showDlgFactionApplyConfirm = true;
    } catch (e) {
      showError(e);
    } finally {
      $factionBusy = false;
    }
  }

  async function handleImportProfile() {
    if ($factionBusy) return;
    $factionOpError = "";
    const path = await open({ multiple: false, filters: [{ name: "Faction editor settings", extensions: ["gwfe"] }] });
    if (!path || Array.isArray(path)) return;

    $factionBusy = true;
    try {
      const item = await importFactionProfile(path);
      await loadFactionProfiles();
      $factionSelectedId = item.id;
    } catch (e) {
      showError(e);
    } finally {
      $factionBusy = false;
    }
  }

  async function handleExportProfile(id: string, name: string) {
    if ($factionBusy) return;
    $factionOpError = "";
    const destPath = await save({
      filters: [{ name: "Faction editor settings", extensions: ["gwfe"] }],
      defaultPath: factionBundleFileName(name),
    });
    if (!destPath) return;

    $factionBusy = true;
    try {
      await exportFactionProfile(id, destPath);
      $factionResult = { ok: true, message: $_("app.factionSettings.exportSaved"), detail: destPath, warnings: [], backupPath: null };
      $showDlgFactionResult = true;
    } catch (e) {
      showError(e);
    } finally {
      $factionBusy = false;
    }
  }

  function handleRenameProfile(id: string, name: string, description: string) {
    $factionOpError = "";
    $factionRenameTarget = { id, name, description };
    $showDlgFactionRename = true;
  }

  function handleDeleteProfile(id: string, name: string) {
    $factionOpError = "";
    $factionDeleteTarget = { id, name };
    $showDlgFactionDelete = true;
  }

</script>

<div class="faction_view">
  <h2>{$_("app.factionSettings.title")}</h2>

  <div class="version-row">
    <span class="version-label">{$_("app.factionSettings.version")}</span>
    <select
      class="version-select"
      disabled={$factionBusy || versionOptions.length === 0}
      value={$factionVersionName}
      onchange={(e) => handleVersionChange((e.currentTarget as HTMLSelectElement).value)}>
      {#each versionOptions as name}
        <option value={name}>{name}</option>
      {/each}
    </select>
  </div>

  {#if versionOptions.length === 0}
    <p class="error">{$_("app.factionSettings.errors.FE_ERR_NO_VERSION")}</p>
  {/if}

  <div class="toolbar">
    <Button size="slim" isDisabled={$factionBusy} onclick={openSaveAsDialog}>
      <Save size={16} style="vertical-align: -3px; margin-right: 6px;" />{$_("app.factionSettings.saveCurrentAs")}
    </Button>
    <Button size="slim" isDisabled={$factionBusy} onclick={handleImportFile}>
      <Upload size={16} style="vertical-align: -3px; margin-right: 6px;" />{$_("app.factionSettings.importAndApply")}
    </Button>
    <Button size="slim" isDisabled={$factionBusy} onclick={handleImportProfile}>{$_("app.factionSettings.importToList")}</Button>
    <Button size="slim" isDisabled={$factionBusy} onclick={openExportCurrentDialog}>
      <Download size={16} style="vertical-align: -3px; margin-right: 6px;" />{$_("app.factionSettings.exportCurrent")}
    </Button>
    <Button size="slim" isRed isDisabled={$factionBusy} onclick={handleResetDefault}>
      <RotateCcw size={16} style="vertical-align: -3px; margin-right: 6px;" />{$_("app.factionSettings.resetDefault")}
    </Button>
  </div>

  {#if $factionOpError}
    <p class="error">{$factionOpError}</p>
  {/if}

  <div class="layout">
    <Scroll value={320} style="flex: 1;">
      {#if $factionProfiles.length === 0}
        <p class="muted">{$_("app.factionSettings.noProfiles")}</p>
      {/if}
      {#each $factionProfiles as item (item.id)}
        <Bg style={$factionSelectedId === item.id ? "outline: 2px solid rgba(61,93,236,0.9); cursor: pointer;" : "cursor: pointer;"}>
          <div
            role="button"
            tabindex="0"
            onclick={() => selectProfile(item.id)}
            onkeydown={(e) => (e.key === "Enter" || e.key === " ") && selectProfile(item.id)}>
            <div class="profile-name">{item.manifest.name}</div>
            {#if item.manifest.author}
              <div class="profile-meta">{item.manifest.author}</div>
            {/if}
            {#if item.manifest.created_at}
              <div class="profile-meta">{new Date(item.manifest.created_at).toLocaleString()}</div>
            {/if}
          </div>
        </Bg>
      {/each}
    </Scroll>

    <div class="details">
      {#if selected}
        <Bg>
          <div class="details-title">{selected.manifest.name}</div>
          {#if selected.manifest.description}
            <p>{selected.manifest.description}</p>
          {/if}
          <FactionSummary manifest={selected.manifest} />

          <div class="actions">
            <Button size="slim" isDisabled={$factionBusy} onclick={() => handleApplyProfile(selected!.id)}>{$_("app.factionSettings.apply")}</Button>
            <Button size="slim" isDisabled={$factionBusy} onclick={() => handleExportProfile(selected!.id, selected!.manifest.name)}>
              <Download size={14} style="vertical-align: -2px; margin-right: 4px;" />{$_("app.btn.export")}
            </Button>
            <Button
              size="slim"
              isDisabled={$factionBusy}
              onclick={() => handleRenameProfile(selected!.id, selected!.manifest.name, selected!.manifest.description)}>
              <Pencil size={14} style="vertical-align: -2px; margin-right: 4px;" />{$_("app.btn.rename")}
            </Button>
            <Button size="slim" isRed isDisabled={$factionBusy} onclick={() => handleDeleteProfile(selected!.id, selected!.manifest.name)}>
              <Trash2 size={14} style="vertical-align: -2px; margin-right: 4px;" />{$_("app.factionSettings.delete")}
            </Button>
          </div>
        </Bg>
      {:else}
        <p class="muted">{$_("app.factionSettings.selectProfile")}</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .faction_view {
    padding: 1.5rem;
    font-family: system-ui, sans-serif;
  }

  h2,
  p,
  div {
    color: white;
  }

  h2 {
    margin: 0 0 1.5rem 0;
  }

  .version-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 1rem;
  }
  .version-label {
    opacity: 0.85;
  }
  .version-select {
    -webkit-app-region: no-drag;
    padding: 0.4rem 0.6rem;
    font-size: 1rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
    min-width: 240px;
  }
  .version-select:focus {
    background-color: rgba(255, 255, 255, 1);
  }
  .version-select:disabled {
    opacity: 0.6;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 1rem;
  }

  .error {
    color: #f55858;
    margin-bottom: 1rem;
  }

  .muted {
    opacity: 0.7;
  }

  .layout {
    display: flex;
    gap: 20px;
  }

  .details {
    flex: 1.4;
  }

  .profile-name {
    font-weight: bold;
  }
  .profile-meta {
    font-size: 0.85em;
    opacity: 0.7;
  }

  .details-title {
    font-weight: bold;
    font-size: 1.1em;
    margin-bottom: 6px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 10px;
  }
</style>
