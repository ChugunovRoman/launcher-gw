<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";

  import { progress, isInProcess, finish, completed, currentFile, processedSize, totalSize, status } from "../store/pack";
  import { providersWasInited } from "../store/main";
  import { choosePath } from "../utils/path";

  import Progress from "../Components/Progress.svelte";
  import Spin from "../Components/Spin.svelte";
  import { getInMb, parseBytes } from "../utils/dwn";

  let packPath = $state("");
  let targetPath = $state("");
  let chunkSize = $state(2000);

  async function chooseSrcPath() {
    await choosePath((selected) => (packPath = selected));
  }
  async function chooseTargetPath() {
    await choosePath((selected) => (targetPath = selected));
  }
  async function startPack() {
    console.log("startPack");
    console.log("startPack, packPath: ", packPath);
    console.log("startPack, targetPath: ", targetPath);
    if (packPath === "" || targetPath === "" || $isInProcess) return;

    $progress = 0;
    $isInProcess = true;
    $finish = false;

    await invoke<AppConfig>("set_pack_paths", { source: packPath, target: targetPath });

    const result = await invoke<string>("create_split_archives", {
      sourceDir: packPath,
      targetPath: targetPath,
      chunkSize,
      excludePatterns: [
        "*.git",
        "*.git/**",
        ".gitlab-ci.yml",
        "*/.gitlab-ci.yml",
        ".editorconfig",
        "*/.editorconfig",
        ".gitignore",
        "*/.gitignore",
        ".gitmodules",
        "*/.gitmodules",
        ".gitconfig",
        "*/.gitconfig",
        ".gitattributes",
        "*/.gitattributes",
        "*.pl",
        "*.sh",
        "utils/**",
        "utils",
        "*.rar",
        ".vscode",
        ".vscode/**",
        "*/.vscode/**",
        "xrLost.exe",
        "xrPlay.ini",
        "packer.exe",
        "gamedata/helpers",
        "gamedata/helpers/**",
        "appdata/logs",
        "appdata/logs/**",
        "appdata/savedgames",
        "appdata/savedgames/**",
        "appdata/screenshots",
        "appdata/screenshots/**",
        "appdata/shaders_cache",
        "appdata/shaders_cache/**",
        "appdata/shaders_cache_oxr",
        "appdata/shaders_cache_oxr/**",
        "appdata/launcherdata",
        "appdata/launcherdata/**",
        "appdata/cdb_cache",
        "appdata/cdb_cache/**",
        "appdata/reports",
        "appdata/reports/**",
        "gamedata/configs/misc/armament/custom",
        "gamedata/configs/misc/armament/custom/**",
        "*JSGME*",
        "*.lnk",
        "*.txt",
        "installer",
        "Compressor",
      ],
    });

    $progress = 100;
    $completed = true;
    console.log("pack result: ", result);
  }

  function getStatusStr(st: number) {
    switch (st) {
      case 0:
        return "addFiles";
      case 1:
        return "compressing";
    }
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
        packPath = config.pack_source_dir;
        targetPath = config.pack_target_dir;
      });
    }
    if ($completed) {
      $completed = false;

      const timeout1 = setTimeout(() => ($finish = true), 500);
      const timeout2 = setTimeout(() => ($isInProcess = false), 1000);
      const timeout3 = setTimeout(() => ($finish = false), 1500);

      onDestroy(() => {
        clearTimeout(timeout1);
        clearTimeout(timeout2);
        clearTimeout(timeout3);
      });
    }
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
    <label class="input-label">{$_("app.pack.chunkSize")}</label>
    <div class="input-row">
      <input type="number" bind:value={chunkSize} placeholder={$_("app.pack.chunkSize")} class="uuid-input" />
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

  <span
    role="button"
    tabindex="0"
    class="packbtn"
    onclick={startPack}
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
