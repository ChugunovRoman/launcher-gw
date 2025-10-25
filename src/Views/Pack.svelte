<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { join } from "@tauri-apps/api/path";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount } from "svelte";

  import { progress, isInProcess, finish, completed } from "../store/pack";

  let packPath = "";
  let targetPath = "";

  $: if ($completed) {
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

  async function choosePath() {
    packPath = await selectFolder(packPath);
  }
  async function chooseTargetPath() {
    targetPath = await selectFolder(targetPath);
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

    const result = await invoke<string>("create_archive", {
      sourceDir: await join(packPath, "*"),
      targetPath: targetPath,
      excludePatterns: [
        "*.git",
        "*.gitlab-ci.yml",
        ".editorconfig",
        ".gitignore",
        ".gitmodules",
        ".gitconfig",
        ".gitattributes",
        "*.pl",
        "*.sh",
        "*.rar",
        "utils",
        ".vscode",
        "xrLost.exe",
        "xrPlay.ini",
        "packer.exe",
        await join("*", ".gitlab-ci.yml"),
        await join("utils", "*"),
        await join("gamedata", "helpers", "*"),
        await join("appdata", "logs", "*"),
        await join("appdata", "savedgames", "*"),
        await join("appdata", "screenshots", "*"),
        await join("appdata", "shaders_cache", "*"),
        await join("appdata", "shaders_cache_oxr", "*"),
        await join("appdata", "launcherdata", "*"),
        await join("appdata", "cdb_cache", "*"),
        await join("appdata", "reports", "*"),
        await join("appdata", "*.ltx"),
        await join("gamedata", "configs", "misc", "armament", "custom", "*"),
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

  async function selectFolder(def: string) {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });

      if (selected) {
        console.log("Выбрана папка:", selected);
        return selected;
      }

      return def;
    } catch (e) {
      console.error("Ошибка при выборе папки:", e);
      return def;
    }
  }

  onMount(async () => {
    const config = await invoke<AppConfig>("get_config");
    const versions = await invoke<Version[]>("get_available_versions");

    console.log("versions: ", versions);

    // await invoke<AppConfig>("start_download_version", { versionId: versions[2].id });

    packPath = config.pack_source_dir;
    targetPath = config.pack_target_dir;
  });
</script>

<div class="pack-view">
  <h2>{$_("app.labels.pack")}</h2>

  <div class="input-group">
    <label class="input-label">{$_("app.pack.source.placeholder")}</label>
    <div class="input-row">
      <input type="text" readonly bind:value={packPath} placeholder={$_("app.pack.source.placeholder")} class="uuid-input" />
      <button type="button" onclick={choosePath} class="choose-btn">
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

  <div class="progress-container">
    <div class="progress-bar" style="width: {Math.min(100, Math.max(0, $progress))}%;"></div>
    <span class="progress-text">{Math.round($progress)}%</span>
  </div>

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
      <svg class="spinner" fill="#FFF" width="24px" height="24px" viewBox="0 0 1000 1000" xmlns="http://www.w3.org/2000/svg"
        ><path
          class="fil0"
          d="M854.569 841.338c-188.268 189.444 -519.825 171.223 -704.157 -13.109 -190.56 -190.56 -200.048 -493.728 -28.483 -695.516 10.739 -12.623 21.132 -25.234 34.585 -33.667 36.553 -22.89 85.347 -18.445 117.138 13.347 30.228 30.228 35.737 75.83 16.531 111.665 -4.893 9.117 -9.221 14.693 -16.299 22.289 -140.375 150.709 -144.886 378.867 -7.747 516.005 152.583 152.584 406.604 120.623 541.406 -34.133 106.781 -122.634 142.717 -297.392 77.857 -451.04 -83.615 -198.07 -305.207 -291.19 -510.476 -222.476l-.226 -.226c235.803 -82.501 492.218 23.489 588.42 251.384 70.374 166.699 36.667 355.204 -71.697 493.53 -11.48 14.653 -23.724 28.744 -36.852 41.948z" /></svg>
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

  .progress-container {
    width: 100%;
    height: 24px;
    background-color: #e0e0e0;
    border-radius: 4px;
    overflow: hidden;
    position: relative;
    margin: 8px 0;
  }
  .progress-bar {
    height: 100%;
    background: linear-gradient(to right, #2196f3, #4caf50);
    transition: width 0.2s ease;
  }
  .progress-text {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    font-size: 14px;
    font-weight: 500;
    color: #333;
    text-shadow:
      0 0 2px #fff,
      0 0 2px #fff;
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

  .spinner {
    width: 16px;
    height: 16px;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    0% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }
</style>
