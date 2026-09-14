<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";

  import {
    progress,
    isInProcess,
    finish,
    completed,
    success,
    error,
    skippedFiles,
    currentFile,
    processedSize,
    totalSize,
    status,
  } from "../store/pack";
  import { configReady } from "../store/main";
  import { choosePath, chooseFilePath } from "../utils/path";
  import { DEFAULT_EXCLUDE_PATTERNS } from "../consts";

  import Progress from "../Components/Progress.svelte";
  import Spin from "../Components/Spin.svelte";
  import { getInMb, parseBytes } from "../utils/dwn";

  let packPath = $state("");
  let targetPath = $state("");
  let chunkSize = $state(2000);
  let exePath = $state("");

  async function chooseSrcPath() {
    await choosePath((selected) => (packPath = selected));
  }
  async function chooseTargetPath() {
    await choosePath((selected) => (targetPath = selected));
  }
  async function chooseExePath() {
    await chooseFilePath((selected) => (exePath = selected));
  }
  type PackResult = {
    skippedFiles: string[];
    totalFilesCount: number;
  };

  async function startPack() {
    if (packPath === "" || targetPath === "" || $isInProcess) return;

    // chunkSize must be a positive integer (MB)
    const chunkSizeInt = Math.floor(Number(chunkSize));
    if (!Number.isFinite(chunkSizeInt) || chunkSizeInt <= 0) return;
    chunkSize = chunkSizeInt;

    $progress = 0;
    $isInProcess = true;
    $finish = false;
    $success = false;
    $error = "";
    $skippedFiles = [];

    try {
      await invoke<AppConfig>("set_pack_paths", { source: packPath, target: targetPath });

      const result = await invoke<PackResult>("create_split_archives", {
        sourceDir: packPath,
        targetPath: targetPath,
        chunkSize,
        exePath: exePath && exePath.length ? exePath : null,
        excludePatterns: DEFAULT_EXCLUDE_PATTERNS,
      });

      $progress = 100;
      // `success` survives the effect below (which immediately clears
      // `completed`), so the result message stays visible.
      $success = true;
      $skippedFiles = result?.skippedFiles ?? [];
      if ($skippedFiles.length) {
        console.warn("pack skipped files: ", $skippedFiles);
      }
      $completed = true;
    } catch (e) {
      // Every packing error (no files, file over the chunk limit, walk failure)
      // used to end up as an unhandled rejection — the user only saw the button
      // reset (item 33).
      console.error("pack failed: ", e);
      $error = e instanceof Error ? e.message : String(e);
      $progress = 0;
      $isInProcess = false;
      $finish = false;
    }
  }

  function getStatusStr(st: number) {
    switch (st) {
      case 0:
        return "addFiles";
      case 1:
        return "compressing";
      case 2:
        return "hashing";
    }
  }

  let finishTimers: ReturnType<typeof setTimeout>[] = [];

  $effect(() => {
    if ($configReady) {
      invoke<AppConfig>("get_config").then((config) => {
        packPath = config.pack_source_dir;
        targetPath = config.pack_target_dir;
      });
    }
    if ($completed) {
      $completed = false;

      for (const t of finishTimers) clearTimeout(t);
      finishTimers = [
        setTimeout(() => ($finish = true), 500),
        setTimeout(() => ($isInProcess = false), 1000),
        setTimeout(() => ($finish = false), 1500),
      ];
    }
  });

  onDestroy(() => {
    for (const t of finishTimers) clearTimeout(t);
  });
</script>

<div class="pack-view">
  <h2>{$_("app.labels.pack")}</h2>

  <div class="input-group">
    <label class="input-label">{$_("app.pack.source.placeholder")}</label>
    <div class="input-row">
      <input type="text" readonly bind:value={packPath} placeholder={$_("app.pack.source.placeholder")} class="uuid-input" />
      <button type="button" onclick={chooseSrcPath} class="choose-btn">
        {$_("app.pack.source.btn")}
      </button>
    </div>
  </div>

  <div class="input-group">
    <label class="input-label">{$_("app.pack.target.placeholder")}</label>
    <div class="input-row">
      <input type="text" readonly bind:value={targetPath} placeholder={$_("app.pack.target.placeholder")} class="uuid-input" />
      <button type="button" onclick={chooseTargetPath} class="choose-btn">
        {$_("app.pack.target.btn")}
      </button>
    </div>
  </div>

  <div class="input-group">
    <label class="input-label">{$_("app.pack.exePath")}</label>
    <div class="input-row">
      <input type="text" readonly bind:value={exePath} placeholder={$_("app.pack.exePath")} class="uuid-input" />
      <button type="button" onclick={chooseExePath} class="choose-btn">
        {$_("app.pack.source.btn")}
      </button>
    </div>
  </div>

  <div class="input-group">
    <label class="input-label">{$_("app.pack.chunkSize")}</label>
    <div class="input-row">
      <input type="number" min="1" step="1" bind:value={chunkSize} placeholder={$_("app.pack.chunkSize")} class="uuid-input" />
    </div>
  </div>

  {#if $isInProcess}
    <div class="meta">
      <span>{$_(`app.pack.${getStatusStr($status)}`)}</span>
      <span>{$_("app.pack.size")}{getInMb($processedSize)}/{getInMb($totalSize)}{$_(`app.common.${parseBytes($totalSize / 1024)[1]}`)}</span>
      <span>{$_("app.pack.file")} {$currentFile}</span>
    </div>
  {/if}

  <Progress progress={$progress} />

  {#if $error}
    <div class="pack-error">{$_("app.pack.failed")}: {$error}</div>
  {/if}

  {#if $success}
    <div class="pack-summary">{$_("app.pack.hashesDone")}</div>
    {#if $skippedFiles.length}
      <div class="pack-warning">{$_("app.pack.skippedFiles", { values: { count: $skippedFiles.length } })}</div>
    {/if}
  {/if}

  <span
    role="button"
    tabindex="0"
    class="packbtn"
    onclick={() => void startPack()}
    class:packbtn__coping={$isInProcess}
    class:packbtn__finish={$finish}
    class:long_t={$finish}>
    {#if $isInProcess}
      {$_("app.pack.packing")}
      <Spin size={16} />
    {:else}
      {$_("app.pack.start")}
    {/if}
  </span>
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }

  .pack-view {
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
  }

  .meta {
    -webkit-app-region: no-drag;
    margin-bottom: 0.5rem;
    text-align: left;
  }

  .pack-summary {
    -webkit-app-region: no-drag;
    margin-top: 0.5rem;
    text-align: left;
    color: #4caf50;
    font-size: 0.85rem;
  }

  .pack-warning {
    -webkit-app-region: no-drag;
    margin-top: 0.25rem;
    text-align: left;
    color: #e9c53d;
    font-size: 0.85rem;
  }

  .pack-error {
    -webkit-app-region: no-drag;
    margin-top: 0.5rem;
    text-align: left;
    color: #ec6161;
    font-size: 0.85rem;
    word-break: break-word;
  }

  .input-group {
    -webkit-app-region: no-drag;
    margin-bottom: 2.5rem;
  }

  .input-label {
    display: block;
    margin-bottom: 0.5rem;
    font-weight: 500;
    color: #fff;
    text-align: left;
  }

  .input-row {
    display: flex;
    gap: 0.75rem;
  }
  .uuid-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .uuid-input:focus {
    background-color: rgba(255, 255, 255, 1);
  }
  .choose-btn {
    -webkit-app-region: no-drag;
    padding: 0.6rem 1.6rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: 0px solid #ccc;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }
  .choose-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }

  .packbtn {
    position: absolute;
    bottom: 50px;
    right: 140px;
    padding: 10px 40px;
    background-color: rgba(61, 93, 236, 0.8);
    transition: background-color 0.15s ease;
    border-radius: 3px;
    -webkit-app-region: no-drag;
  }
  .packbtn:hover {
    cursor: pointer;
    background-color: rgba(61, 93, 236, 1);
  }
  .packbtn__coping {
    cursor: default;
    background-color: rgba(233, 236, 61, 0.8);
  }
  .packbtn__coping:hover {
    cursor: default;
    background-color: rgba(233, 236, 61, 0.8);
  }
  .packbtn__finish {
    cursor: default;
    background-color: rgba(61, 236, 128, 0.8);
  }
  .packbtn__finish:hover {
    cursor: default;
    background-color: rgba(61, 236, 128, 0.8);
  }
  .long_t {
    transition: background-color 1s ease;
  }

  .packbtn {
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
