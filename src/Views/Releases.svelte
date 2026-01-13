<!-- ReleasesView.svelte -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { providersWasInited } from "../store/main";
  import {
    showUploading,
    inProcess,
    versions,
    logText,
    releaseName,
    releasePath,
    filesPerCommit,
    totalFiles,
    uploadedFiles,
    uploadFilesMap,
  } from "../store/upload";
  import { choosePath } from "../utils/path";

  import Progress from "../Components/Progress.svelte";
  import Button from "../Components/Button.svelte";
  import Spin from "../Components/Spin.svelte";
  import { getInMb, parseBytes } from "../utils/dwn";

  let expandedIndex = $state<number | null>(null);

  async function fetchVersions() {
    const fetched = await invoke<Version[]>("get_available_versions");

    versions.set(fetched.filter((r) => r.name !== $releaseName));
  }

  async function handleCreateRelease(event: Event) {
    event.stopPropagation();

    if (!$releaseName.trim() || !$releasePath.trim()) return;

    showUploading.set(true);
    inProcess.set(true);
    expandedIndex = -1;

    console.log("handleCreateRelease, ", {
      newReleaseName: $releaseName,
      newReleasePath: $releasePath,
    });

    await invoke<void>("create_release_repos", {
      name: $releaseName,
      path: $releasePath,
    });

    await startUploadingRelease();

    setTimeout(() => {
      showUploading.set(false);
      inProcess.set(false);
    }, 2000);
  }

  async function startUploadingRelease() {
    await invoke<void>("upload_v2_release", {
      name: $releaseName,
      path: $releasePath,
    });

    await fetchVersions();
  }

  async function chooseNewReleasePath(event: Event) {
    event.stopPropagation();

    await choosePath((selected) => releasePath.set(selected));
  }

  async function handleContinueUploading() {
    inProcess.set(true);

    updateFilesCounts();

    await invoke<void>("continue_upload");

    setTimeout(() => {
      showUploading.set(false);
      inProcess.set(false);
      expandedIndex = null;
    }, 2000);

    await fetchVersions();
  }

  function updateFilesCounts() {
    invoke<RepoSyncState | null>("get_upload_manifest").then((manifest) => {
      if (manifest) {
        totalFiles.set(manifest.total_files_count);
        uploadedFiles.set(manifest.uploaded_files_count);
      }
    });
  }

  function toggleExpand(index: number) {
    expandedIndex = expandedIndex === index ? null : index;
  }

  $effect(() => {
    if ($providersWasInited) {
      invoke<AppConfig>("get_config").then((config) => {
        if (!$showUploading && !!config.progress_upload) {
          showUploading.set(true);
          releaseName.set(config.progress_upload.name);

          invoke<RepoSyncState | null>("get_upload_manifest").then((manifest) => {
            if (manifest) {
              totalFiles.set(manifest.total_files_count);
              uploadedFiles.set(manifest.uploaded_files_count);
            }
          });
        }
      });
    }
  });
</script>

<div class="releases-view">
  <h2>{$_("app.labels.releases")}</h2>

  <div class="releases-scroll">
    <!-- Элемент для добавления нового релиза -->
    <div class="release-item add-item">
      <div class="header" role="button" tabindex="0" onclick={() => toggleExpand(-2)}>
        <span class="plus-icon">+</span>
        <span class="placeholder-text">{$_("app.releases.add")}</span>
      </div>
      {#if expandedIndex === -2}
        <div class="expanded-content">
          <div class="one-row">
            <div class="input-group">
              <label class="input-label">{$_("app.releases.name")}</label>
              <input type="text" bind:value={$releaseName} placeholder={$_("app.releases.name")} class="release-input" />
            </div>
            <div class="input-group">
              <label class="input-label">{$_("app.releases.filespc")}</label>
              <input type="text" bind:value={$filesPerCommit} placeholder={$_("app.releases.filespc")} class="release-input" />
            </div>
          </div>
          <div class="input-group">
            <label class="input-label">{$_("app.releases.path")}</label>
            <div class="input-row">
              <input type="text" readonly bind:value={$releasePath} placeholder={$_("app.releases.path")} class="release-input" />
              <button type="button" onclick={chooseNewReleasePath} class="choose-btn">
                {$_("app.releases.browse")}
              </button>
            </div>
          </div>
          <button type="button" onclick={handleCreateRelease} class="create-btn">
            {$_("app.releases.create")}
          </button>
        </div>
      {/if}
    </div>

    {#if $showUploading}
      <div class="release-item uplaod-item" onclick={() => toggleExpand(-1)}>
        <div class="header" role="button" tabindex="0">
          <span class="plus-icon">
            {#if $inProcess}
              <Spin size={16} />
            {:else}
              <svg width="20" height="20" viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg">
                <path d="M24 5L44 43H4L24 5Z" fill="none" stroke="rgba(233, 236, 61, 1)" stroke-width="4" stroke-linejoin="round" />
                <circle cx="24" cy="34" r="3" fill="rgba(233, 236, 61, 1)" />
                <rect x="22" y="18" width="4" height="10" fill="rgba(233, 236, 61, 1)" rx="2" />
              </svg>
            {/if}
          </span>
          {#if $inProcess}
            <span class="placeholder-text">{$_("app.releases.uploading")}, {$_("app.releases.uploaded")}</span>
          {:else}
            <span class="placeholder-text">{$_("app.releases.stoped")} ({$releaseName})</span>
          {/if}
          {#if !$inProcess}
            <button type="button" onclick={handleContinueUploading} class="continue-btn">
              {$_("app.releases.continue")}
            </button>
          {/if}
        </div>
        {#if expandedIndex === -1}
          <div class="expanded-content">
            {#each $uploadFilesMap as [name, progress], i}
              <div class="file-row">
                <span>{name}</span>

                <Progress height={12} maxWidth="1fr - 300px" progress={progress.progress} showPercents={false} />

                <span style="justify-self: end;"
                  >{parseBytes(progress.file_uploaded_size)[0]}
                  {$_(`app.common.${parseBytes(progress.file_uploaded_size)[1]}`)} / {parseBytes(progress.file_total_size)[0]}
                  {$_(`app.common.${parseBytes(progress.file_total_size)[1]}`)}</span>

                <span style="justify-self: end;">{progress.speedValue} {progress.sfxValue}</span>
              </div>
            {/each}
            {#each $logText as text, i}
              <span class="log-text">{text}</span>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Список существующих релизов -->
    {#each $versions as version, i}
      <div class="release-item" onclick={() => toggleExpand(i)}>
        <div class="header">
          <span class="version-name">{version.name}</span>
        </div>
        {#if expandedIndex === i}
          <div class="expanded-content installed-status">
            <span class="status-icon">✓</span>
            <span class="status-text">{$_("app.releases.installed")}</span>
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  h2 {
    margin-bottom: 4rem;
  }

  .releases-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 1.5rem;
    margin: 0 auto;
    font-family: system-ui, sans-serif;
  }

  .file-row {
    display: grid;
    grid-template-columns: 120px 1fr 140px 100px;
  }

  .releases-scroll {
    overflow-y: auto;
    -webkit-app-region: no-drag;
    height: calc(100vh - 220px);
    padding-right: 20px;
  }
  .releases-scroll::-webkit-scrollbar {
    width: 12px;
  }
  .releases-scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .releases-scroll::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .releases-scroll::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .releases-scroll::-webkit-scrollbar-button {
    display: none;
  }

  .release-item {
    -webkit-app-region: no-drag;
    background-color: rgba(40, 40, 40, 0.6);
    border-radius: 6px;
    margin-bottom: 1rem;
    overflow: hidden;
    cursor: pointer;
    transition: background-color 0.2s ease;
  }
  .release-item:hover {
    background-color: rgba(50, 50, 50, 0.7);
  }

  .header {
    display: flex;
    align-items: center;
    padding: 1rem 1.25rem;
    gap: 0.75rem;
  }

  .plus-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    font-size: 1.25rem;
    color: #4caf50;
    font-weight: bold;
  }

  .placeholder-text {
    color: #aaa;
    font-style: italic;
  }

  .version-name {
    color: white;
    font-weight: 500;
  }

  .expanded-content {
    padding: 1rem 1.25rem 1.25rem;
    border-top: 1px solid rgba(255, 255, 255, 0.1);
    overflow-y: auto;
    max-height: 500px;
  }
  .expanded-content::-webkit-scrollbar {
    width: 12px;
  }
  .expanded-content::-webkit-scrollbar-track {
    background: transparent;
  }
  .expanded-content::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .expanded-content::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .expanded-content::-webkit-scrollbar-button {
    display: none;
  }

  .installed-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-icon {
    color: #4caf50;
    font-size: 1.2rem;
  }

  .status-text {
    color: #4caf50;
    font-weight: 500;
  }

  .log-text {
    display: block;
    color: white;
    text-align: left;
    font-size: 0.8rem;
    color: #aaa;
    font-family: monospace;
  }

  .input-group {
    margin-bottom: 1.25rem;
  }
  .one-row {
    display: grid;
    grid-template-columns: 1fr 300px;
  }

  .input-label {
    display: block;
    margin-bottom: 0.5rem;
    color: #fff;
    font-weight: 500;
  }
  .input-row {
    display: flex;
    gap: 0.75rem;
  }

  .release-input {
    -webkit-app-region: no-drag;
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #555;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
    width: 95%;
  }
  .release-input:focus {
    background-color: rgba(255, 255, 255, 1);
    outline: none;
  }

  .choose-btn {
    -webkit-app-region: no-drag;
    padding: 0.5rem 1rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: none;
    border-radius: 3px;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }
  .choose-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }

  .create-btn {
    -webkit-app-region: no-drag;
    padding: 0.6rem 1.5rem;
    color: white;
    background-color: rgba(76, 175, 80, 0.8);
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-weight: 500;
    transition: background-color 0.15s ease;
  }
  .create-btn:hover {
    background-color: rgba(76, 175, 80, 1);
  }

  .continue-btn {
    -webkit-app-region: no-drag;
    padding: 0.3rem 1rem;
    color: white;
    background-color: rgba(76, 175, 80, 0.8);
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: background-color 0.15s ease;
    margin-left: auto;
  }

  .continue-btn:hover {
    background-color: rgba(76, 175, 80, 1);
  }
</style>
