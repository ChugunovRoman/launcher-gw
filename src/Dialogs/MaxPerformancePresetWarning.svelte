<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import Checkbox from "../Components/Checkbox.svelte";
  import { appConfig, showDlgMaxPerformancePresetWarning } from "../store/main";

  let dontShowAgain = $state(false);

  async function persistDontShowIfNeeded() {
    if (!dontShowAgain) return;
    await invoke<void>("set_hide_max_perf_preset_warning", { value: true });
    appConfig.update((cfg) => ({ ...cfg, hide_max_perf_preset_warning: true }));
  }

  function handleClose() {
    persistDontShowIfNeeded();
    dontShowAgain = false;
  }

  async function okHandler() {
    await persistDontShowIfNeeded();
    dontShowAgain = false;
    $showDlgMaxPerformancePresetWarning = false;
  }
</script>

<Modal bind:showModal={$showDlgMaxPerformancePresetWarning} onClose={handleClose}>
  {#snippet header()}
    <span>{$_("app.dlg.attention")}</span>
  {/snippet}

  <p>{$_("app.dlg.presetMaxPerformanceWarning")}</p>

  {#snippet footer()}
    <Checkbox bind:checked={dontShowAgain}>
      {$_("app.dlg.dontShowAgain")}
    </Checkbox>

    <Button isRed onclick={okHandler}>{$_("app.dlg.ok")}</Button>
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

  footer {
    justify-content: space-between;
  }
</style>
