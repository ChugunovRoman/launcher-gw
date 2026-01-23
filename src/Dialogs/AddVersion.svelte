<script lang="ts">
  import { _ } from "svelte-i18n";
  import { open } from "@tauri-apps/plugin-dialog";
  import { fetchLocalVersions, localVersions, showDlgAddVersion } from "../store/main";

  import Modal from "./Base.svelte";
  import Button from "../Components/Button.svelte";
  import Bg from "../Components/Bg.svelte";
  import { join } from "@tauri-apps/api/path";
  import { invoke } from "@tauri-apps/api/core";
  import { selectedVersion } from "../store/upload";
  import { get } from "svelte/store";

  let { maxWidth = "500px" } = $props();

  let newVersionName = $state<string>("");
  let newVersionPath = $state<string>("");
  let newVersionBinPath = $state<string>("");
  let newVersionBinPathCheck = $state<boolean>(false);
  let newVersionFsgamePath = $state<string>("");
  let newVersionFsgamePathCheck = $state<boolean>(false);
  let newVersionUserltxPath = $state<string>("");
  let newVersionUserltxPathCheck = $state<boolean>(false);

  function handleClose() {
    console.log("Dlg was closed");
  }
  function clearHandler() {
    newVersionName = "";
  }
  async function handleAdd() {
    await invoke("add_installed_version_from_ui", {
      name: newVersionName,
      path: newVersionPath,
      enginePath: newVersionBinPath,
      fsgamePath: newVersionFsgamePath,
      userltxPath: newVersionUserltxPath,
    });

    setTimeout(async () => {
      if (localVersions.size() === 0) {
        selectedVersion.set(undefined);
      }

      await fetchLocalVersions();

      if (!get(selectedVersion)) {
        selectedVersion.set([...get(localVersions).keys()][0]);
        await invoke<void>("set_current_game_version", { versionName: get(selectedVersion) });
      }

      $showDlgAddVersion = false;
    }, 150);
  }
  async function choosePathHandler() {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (!selected) {
      return;
    }

    newVersionPath = selected;
    newVersionBinPath = await join(newVersionPath, "bin", "xrEngine.exe");
    const binFileExists = await invoke("check_file_exists", { path: newVersionBinPath });
    if (!binFileExists) {
      newVersionBinPathCheck = true;
    }
    newVersionFsgamePath = await join(newVersionPath, "fsgame.ltx");
    const fsgameFileExists = await invoke("check_file_exists", { path: newVersionFsgamePath });
    if (!fsgameFileExists) {
      newVersionFsgamePathCheck = true;
    }
    newVersionUserltxPath = await join(newVersionPath, "appdata", "user.ltx");
    const userltxFileExists = await invoke("check_file_exists", { path: newVersionUserltxPath });
    if (!userltxFileExists) {
      newVersionUserltxPathCheck = true;
    }
  }
  async function chooseExeHandler() {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "*.exe",
          extensions: ["exe"],
        },
      ],
    });
    if (!selected) {
      return;
    }

    newVersionBinPath = selected;
    newVersionBinPathCheck = false;
  }
  async function chooseFsgameHandler() {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "fsgame.ltx",
          extensions: ["ltx"],
        },
      ],
    });
    if (!selected) {
      return;
    }

    newVersionFsgamePath = selected;
    newVersionFsgamePathCheck = false;
  }
  async function chooseUserltxHandler() {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        {
          name: "user.ltx",
          extensions: ["ltx"],
        },
      ],
    });
    if (!selected) {
      return;
    }

    newVersionUserltxPath = selected;
    newVersionUserltxPathCheck = false;
  }
</script>

<Modal bind:showModal={$showDlgAddVersion} onClose={handleClose} {maxWidth}>
  {#snippet header()}
    <span>{$_("app.dlg.addVersionTitle")}</span>
  {/snippet}

  <Bg>
    <span>{$_("app.labels.newVersionName")}</span>
    <div class="input-row">
      <input type="text" bind:value={newVersionName} placeholder={$_("app.labels.newVersionName")} class="launch-args-input" />
      <Button onclick={clearHandler}>{$_("app.btn.clear")}</Button>
    </div>
  </Bg>

  <Bg>
    <span>{$_("app.labels.newVersionPath")}</span>
    <div class="input-row">
      <input readonly type="text" bind:value={newVersionPath} placeholder={$_("app.labels.newVersionPath")} class="launch-args-input" />
      <Button onclick={choosePathHandler}>{$_("app.btn.choose")}</Button>
    </div>
  </Bg>

  <Bg>
    <span>{$_("app.labels.newVersionBinPath")}</span>
    <div class="input-row">
      <input readonly type="text" bind:value={newVersionBinPath} placeholder={$_("app.labels.newVersionBinPath")} class="launch-args-input" />
      <Button onclick={chooseExeHandler}>{$_("app.btn.choose")}</Button>
    </div>
    {#if newVersionBinPathCheck}
      <span class="input-label-2">{$_("app.input.checks.newVersionBinPathExists")}</span>
    {/if}
  </Bg>

  <Bg>
    <span>{$_("app.labels.newVersionFsgamePath")}</span>
    <div class="input-row">
      <input readonly type="text" bind:value={newVersionFsgamePath} placeholder={$_("app.labels.newVersionFsgamePath")} class="launch-args-input" />
      <Button onclick={chooseFsgameHandler}>{$_("app.btn.choose")}</Button>
    </div>
    {#if newVersionFsgamePathCheck}
      <span class="input-label-2">{$_("app.input.checks.newVersionFsgamePathExists")}</span>
    {/if}
  </Bg>

  <Bg>
    <span>{$_("app.labels.newVersionUserltxPath")}</span>
    <div class="input-row">
      <input readonly type="text" bind:value={newVersionUserltxPath} placeholder={$_("app.labels.newVersionUserltxPath")} class="launch-args-input" />
      <Button onclick={chooseUserltxHandler}>{$_("app.btn.choose")}</Button>
    </div>
    {#if newVersionUserltxPathCheck}
      <span class="input-label-2">{$_("app.input.checks.newVersionUserltxPathExists")}</span>
    {/if}
  </Bg>

  {#snippet footer()}
    <Button onclick={handleAdd}>{$_("app.dlg.add")}</Button>
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

  .input-row {
    display: flex;
    gap: 0.75rem;
  }

  .launch-args-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
  }
  .launch-args-input:focus {
    background-color: rgba(255, 255, 255, 1);
  }

  .input-label-2 {
    display: block;
    margin-bottom: 0.5rem;
    color: #f55858;
  }
</style>
