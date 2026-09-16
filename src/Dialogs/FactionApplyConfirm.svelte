<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import FactionSummary from "../Components/FactionSummary.svelte";
  import {
    showDlgFactionApplyConfirm,
    factionApplyContext,
    factionBusy,
    factionResult,
    showDlgFactionResult,
    showDlgFactionGameRunning,
  } from "../store/factionSettings";
  import {
    importFactionBundle,
    applyFactionProfile,
    resetFactionDefaults,
    factionErrorKey,
    factionMessageDetail,
    isFactionGameRunningError,
    loadFactionProfiles,
  } from "../lib/factionSettings";

  function close() {
    $showDlgFactionApplyConfirm = false;
    $factionApplyContext = undefined;
  }

  async function confirm() {
    // `Button isDisabled` is visual only — a second click must not start a
    // second apply on top of the first one's half-written state.
    if ($factionBusy) return;
    const ctx = $factionApplyContext;
    if (!ctx) return;

    $factionBusy = true;
    try {
      const result =
        ctx.kind === "importFile"
          ? await importFactionBundle(ctx.path)
          : ctx.kind === "applyProfile"
            ? await applyFactionProfile(ctx.id)
            : await resetFactionDefaults();

      if (result.outcome === "applied") {
        $factionResult = { ok: true, message: $_("app.factionSettings.applied"), warnings: result.warnings ?? [], backupPath: result.backupPath };
      } else {
        // Failed/RolledBack: `warnings[0]` is `FE_ERR_APPLY_FAILED: <reason>`.
        const raw = result.warnings?.[0] ?? "";
        const headline = result.outcome === "rolledBack" ? $_("app.factionSettings.rolledBack") : $_("app.factionSettings.applyFailed");
        $factionResult = { ok: false, message: headline, detail: factionMessageDetail(raw), warnings: [], backupPath: result.backupPath };
      }
    } catch (err) {
      console.error("faction apply failed:", err);
      if (isFactionGameRunningError(err)) {
        // The game started between the pre-check and "Да".
        $factionBusy = false;
        close();
        $showDlgFactionGameRunning = true;
        return;
      }
      $factionResult = { ok: false, message: $_(factionErrorKey(err)), detail: factionMessageDetail(String(err)), warnings: [], backupPath: null };
    } finally {
      $factionBusy = false;
    }

    close();
    $showDlgFactionResult = true;
    try {
      await loadFactionProfiles();
    } catch (e) {
      console.error("loadFactionProfiles after apply failed:", e);
    }
  }
</script>

<Modal bind:showModal={$showDlgFactionApplyConfirm} onClose={close}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  {#if $factionApplyContext?.kind === "importFile" || $factionApplyContext?.kind === "applyProfile"}
    <p class="name">{$factionApplyContext.manifest.name}</p>
    {#if $factionApplyContext.manifest.description}
      <p class="muted">{$factionApplyContext.manifest.description}</p>
    {/if}
    <FactionSummary manifest={$factionApplyContext.manifest} warnings={$factionApplyContext.warnings} />
    <p>{$_("app.factionSettings.confirmReplace")}</p>
  {:else if $factionApplyContext?.kind === "resetDefault"}
    <p>{$_("app.factionSettings.confirmReset")}</p>
    <p>{$_("app.factionSettings.confirmReplace")}</p>
  {/if}

  {#snippet footer()}
    <Button isRed isDisabled={$factionBusy} onclick={confirm}>{$_("app.dlg.yes")}</Button>
    <Button isDisabled={$factionBusy} onclick={close}>{$_("app.dlg.no")}</Button>
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
  .name {
    font-weight: bold;
  }
  .muted {
    opacity: 0.7;
    font-size: 0.9em;
  }
</style>
