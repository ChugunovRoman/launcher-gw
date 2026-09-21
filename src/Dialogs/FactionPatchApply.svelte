<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import {
    showDlgFactionPatchApply,
    factionPatchContext,
    factionBusy,
    factionResult,
    showDlgFactionResult,
    showDlgFactionGameRunning,
  } from "../store/factionSettings";
  import {
    applyFactionPatch,
    factionErrorKey,
    factionFieldKey,
    factionMessageDetail,
    isFactionGameRunningError,
  } from "../lib/factionSettings";
  import { fetchLocalVersions } from "../store/main";

  // Props the player wants applied. Every prop starts ticked on each open —
  // unticking one drops it in every section of the fragment at once.
  let selected = $state<Set<string>>(new Set());

  // `pre` so the ticks are in place before the list is rendered — a plain
  // `$effect` runs after paint, which would flash an empty list and a disabled
  // confirm button for a frame.
  $effect.pre(() => {
    const ctx = $factionPatchContext;
    selected = new Set(ctx?.fields ?? []);
  });

  // A Set is not deeply reactive in Svelte 5, so replace it to trigger a rerender.
  function toggle(field: string) {
    const next = new Set(selected);
    if (next.has(field)) next.delete(field);
    else next.add(field);
    selected = next;
  }

  function toggleAll() {
    const fields = $factionPatchContext?.fields ?? [];
    selected = selected.size === fields.length ? new Set() : new Set(fields);
  }

  function close() {
    $showDlgFactionPatchApply = false;
    $factionPatchContext = undefined;
  }

  async function confirm() {
    // `Button isDisabled` is visual only — a second click must not start a
    // second apply on top of the first one's half-written state.
    if ($factionBusy) return;
    const ctx = $factionPatchContext;
    if (!ctx) return;

    // Filter the original list so the order the player saw is preserved.
    const fields = ctx.fields.filter((f) => selected.has(f));
    if (fields.length === 0) return;

    $factionBusy = true;
    try {
      const result = await applyFactionPatch(ctx.versionName, ctx.patchName, fields);

      if (result.outcome === "applied") {
        // `applied === 0` means the fragment had nothing to write here (the
        // player's config lacks those sections); say so instead of claiming
        // settings were changed.
        const message = result.applied > 0 ? $_("app.factionSettings.patchApplied") : $_("app.factionSettings.patchNothingToApply");
        $factionResult = { ok: true, message, warnings: result.warnings ?? [], backupPath: result.backupPath };
      } else {
        // Failed/RolledBack: `warnings[0]` is `FE_ERR_APPLY_FAILED: <reason>`.
        const raw = result.warnings?.[0] ?? "";
        const headline = result.outcome === "rolledBack" ? $_("app.factionSettings.rolledBack") : $_("app.factionSettings.applyFailed");
        $factionResult = { ok: false, message: headline, detail: factionMessageDetail(raw), warnings: [], backupPath: result.backupPath };
      }
    } catch (err) {
      console.error("faction patch apply failed:", err);
      if (isFactionGameRunningError(err)) {
        // The game started between opening this dialog and "Да".
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

    // The marker now carries `fe_applied_at`; refresh so the patch row shows
    // the "already applied" hint without reopening the screen.
    try {
      await fetchLocalVersions();
    } catch (e) {
      console.error("fetchLocalVersions after patch apply failed:", e);
    }
  }
</script>

<Modal bind:showModal={$showDlgFactionPatchApply} onClose={close}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  {#if $factionPatchContext}
    <p class="name">{$factionPatchContext.patchName}</p>

    {#if !$factionPatchContext.hasPlayerConfig}
      <!-- The editor was never saved here: the patch already shipped the new
           reference config, which is what the game reads in that case. -->
      <p>{$_("app.factionSettings.warnings.FE_WARN_PATCH_NO_CONFIG")}</p>
    {:else}
      <p>{$_("app.factionSettings.patchChanged")}</p>

      <div class="fields-head">
        <span class="muted">{$_("app.factionSettings.patchPickFields")}</span>
        <button type="button" class="link-btn" onclick={toggleAll}>
          {selected.size === $factionPatchContext.fields.length
            ? $_("app.factionSettings.patchSelectNone")
            : $_("app.factionSettings.patchSelectAll")}
        </button>
      </div>

      <div class="fields">
        {#each $factionPatchContext.fields as field}
          <label class="field-row">
            <input type="checkbox" checked={selected.has(field)} onchange={() => toggle(field)} />
            <span>{$_(factionFieldKey(field), { default: field })}</span>
          </label>
        {/each}
      </div>

      <p>{$_("app.factionSettings.patchApplyAsk")}</p>
      <p class="muted">{$_("app.factionSettings.patchKeptOnDisk")}</p>
      {#if $factionPatchContext.appliedAt}
        <p class="muted">{$_("app.factionSettings.patchAppliedBefore")}</p>
      {/if}
    {/if}
  {/if}

  {#snippet footer()}
    {#if $factionPatchContext?.hasPlayerConfig}
      <Button isRed isDisabled={$factionBusy || selected.size === 0} onclick={confirm}>{$_("app.dlg.yes")}</Button>
      <Button isDisabled={$factionBusy} onclick={close}>{$_("app.dlg.no")}</Button>
    {:else}
      <Button onclick={close}>{$_("app.dlg.ok")}</Button>
    {/if}
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
  .fields-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    padding-bottom: 6px;
  }
  .link-btn {
    background: none;
    border: none;
    padding: 0;
    color: #4aa3ff;
    font-size: 0.85em;
    cursor: pointer;
    text-decoration: underline;
    white-space: nowrap;
  }
  .link-btn:hover {
    color: #7bbcff;
  }
  .fields {
    max-height: 260px;
    overflow-y: auto;
    margin-bottom: 12px;
    padding-right: 6px;
  }
  .field-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
    cursor: pointer;
  }
  .field-row span {
    font-size: 0.95em;
  }
</style>
