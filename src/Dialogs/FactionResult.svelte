<script lang="ts">
  import { _ } from "svelte-i18n";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import { showDlgFactionResult, factionResult } from "../store/factionSettings";
  import { factionWarningKey, factionMessageDetail } from "../lib/factionSettings";

  function close() {
    $showDlgFactionResult = false;
  }
</script>

<Modal bind:showModal={$showDlgFactionResult} onClose={close}>
  {#snippet header()}
    <span>{$factionResult?.ok ? $_("app.factionSettings.resultOkTitle") : $_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$factionResult?.message}</p>

  {#if $factionResult?.detail}
    <p class="muted detail">{$factionResult.detail}</p>
  {/if}

  {#if $factionResult?.backupPath}
    <p class="muted">{$_("app.factionSettings.backupSaved")}: {$factionResult.backupPath}</p>
  {/if}

  {#if $factionResult?.warnings?.length}
    <ul>
      {#each $factionResult.warnings as w}
        <li>
          {$_(factionWarningKey(w))}
          {#if factionMessageDetail(w)}: {factionMessageDetail(w)}{/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#snippet footer()}
    <Button onclick={close}>{$_("app.dlg.ok")}</Button>
  {/snippet}
</Modal>

<style>
  span,
  p,
  li {
    color: white;
  }
  p {
    padding-bottom: 10px;
  }
  .muted {
    opacity: 0.7;
    font-size: 0.9em;
    word-break: break-all;
  }
  .detail {
    white-space: pre-line;
  }
  ul {
    margin: 0 0 10px 0;
    padding-left: 20px;
  }
</style>
