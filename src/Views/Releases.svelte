<!-- ReleasesView.svelte -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { providersWasInited } from "../store/main";
  import {
    showUploading,
    inProcess,
    versions,
    logText,
    releaseName,
    releasePath,
    totalFiles,
    uploadedFiles,
    uploadFilesMap,
  } from "../store/upload";
  import { choosePath } from "../utils/path";
  import { DEFAULT_EXCLUDE_PATTERNS } from "../consts";

  import Progress from "../Components/Progress.svelte";
  import Button from "../Components/Button.svelte";
  import Spin from "../Components/Spin.svelte";
  import { getInMb, parseBytes, formatSpeedBytesPerSec } from "../utils/dwn";

  let expandedIndex = $state<number | null>(null);

  // --- Patch collection (stage 1 of partial updates) ---
  let patchSourcePath = $state("");
  let collectingPatch = $state(false);
  let patchResult = $state<PatchCollectResult | null>(null);
  let patchError = $state("");
  let patchExcludePatterns = $state<string[]>([...DEFAULT_EXCLUDE_PATTERNS]);
  let patchExcludeText = $state("");

  function repoStatusClass(status: string): string {
    switch (status) {
      case "collected":
        return "repo-status collected";
      case "error":
        return "repo-status error";
      default:
        return "repo-status skipped";
    }
  }

  async function choosePatchSourcePath(event: Event) {
    event.stopPropagation();

    await choosePath((selected) => {
      patchSourcePath = selected;
      invoke("set_patch_source_dir", { source: selected });
    });
  }

  async function handleCollectPatch(event: Event) {
    event.stopPropagation();

    if (!patchSourcePath.trim() || collectingPatch) return;

    // Sync textarea -> patterns array before collecting.
    syncExcludeTextToPatterns();

    collectingPatch = true;
    patchError = "";
    patchResult = null;

    try {
      patchResult = await invoke<PatchCollectResult>("collect_patch", {
        sourceDir: patchSourcePath,
        excludePatterns: patchExcludePatterns,
      });
      // Persist the source path for next session.
      invoke("set_patch_source_dir", { source: patchSourcePath });
      // Prefill the per-version "add patch" form with the collected folder.
      lastPatchUploadPath = patchResult.patch_dir;
      // If a version is expanded, also update its state directly.
      if (expandedIndex !== null && expandedIndex >= 0) {
        const vn = $versions[expandedIndex]?.name;
        if (vn) updateUploadState(vn, (s) => { s.uploadPath = patchResult!.patch_dir; });
      }
      invoke("set_patch_upload_dir", { path: patchResult.patch_dir });
    } catch (e) {
      console.error("handleCollectPatch failed:", e);
      patchError = String(e);
    } finally {
      collectingPatch = false;
    }
  }

  // --- Patch upload (stage 2 of partial updates) ---
  // Per-version upload state to prevent cross-version interference.
  interface PatchUploadState {
    uploadPath: string;
    uploadName: string;
    uploading: boolean;
    error: string;
    log: string[];
    files: Map<string, UploadFileData>;
    result: PatchUploadResult | null;
  }
  const defaultUploadState: PatchUploadState = {
    uploadPath: "",
    uploadName: "",
    uploading: false,
    error: "",
    log: [],
    files: new Map(),
    result: null,
  };
  let patchUploadStates = $state<Map<string, PatchUploadState>>(new Map());
  // Tracks which version is currently uploading (for event routing).
  let activeUploadVersion = $state<string | null>(null);
  // Global default upload path (from collect or config).
  let lastPatchUploadPath = $state("");

  /// Non-mutating read for template use — returns existing state or a static default.
  function readUploadState(versionName: string): PatchUploadState {
    return patchUploadStates.get(versionName) ?? defaultUploadState;
  }

  /// Creates the entry if missing (mutation — only in event handlers, never in template).
  function ensureUploadState(versionName: string): PatchUploadState {
    let state = patchUploadStates.get(versionName);
    if (!state) {
      state = {
        uploadPath: lastPatchUploadPath,
        uploadName: "",
        uploading: false,
        error: "",
        log: [],
        files: new Map(),
        result: null,
      };
      patchUploadStates = new Map(patchUploadStates).set(versionName, state);
    }
    return state;
  }

  function updateUploadState(versionName: string, updater: (s: PatchUploadState) => void) {
    const state = ensureUploadState(versionName);
    updater(state);
    // Trigger reactivity by creating a new Map reference.
    patchUploadStates = new Map(patchUploadStates);
  }

  // Subscribe to the patch upload events (kept separate from upload-v2 ones).
  $effect(() => {
    const unlistenLog = listen<string>("patch-upload-log", (e) => {
      const vn = activeUploadVersion;
      if (!vn) return;
      updateUploadState(vn, (s) => {
        s.log = [...s.log.slice(-30), e.payload];
      });
    });
    const unlistenProgress = listen<UploadProgressPayload>("patch-upload-progress", (e) => {
      const vn = activeUploadVersion;
      if (!vn) return;
      const p = e.payload;
      // Keep speedValue numeric (UploadFileData.speedValue is number) and use the
      // shared formatter for consistent units, same as the upload-v2 flow.
      const [speedValue, sfxValue] = formatSpeedBytesPerSec(p.speed);
      updateUploadState(vn, (s) => {
        s.files = new Map(s.files).set(p.file_name, {
          file_uploaded_size: p.file_uploaded_size,
          file_total_size: p.file_total_size,
          progress: p.file_total_size > 0 ? (p.file_uploaded_size / p.file_total_size) * 100 : 0,
          speedValue,
          sfxValue,
        });
      });
    });

    return () => {
      unlistenLog.then((u) => u());
      unlistenProgress.then((u) => u());
    };
  });

  async function choosePatchUploadPath(event: Event, versionName: string) {
    event.stopPropagation();

    await choosePath((selected) => {
      updateUploadState(versionName, (s) => { s.uploadPath = selected; });
      invoke("set_patch_upload_dir", { path: selected });
    });
  }

  async function handleUploadPatch(event: Event, releaseNameStr: string) {
    event.stopPropagation();

    const state = ensureUploadState(releaseNameStr);
    if (!state.uploadPath.trim() || !state.uploadName.trim() || state.uploading) return;

    activeUploadVersion = releaseNameStr;
    updateUploadState(releaseNameStr, (s) => {
      s.uploading = true;
      s.error = "";
      s.result = null;
      s.log = [];
      s.files = new Map();
    });

    // Reuse collect results (deleted files / base tag) when uploading the
    // folder produced by the collector in this session.
    const fromCollect = patchResult !== null && patchResult.patch_dir === state.uploadPath;

    try {
      const result = await invoke<PatchUploadResult>("upload_patch", {
        name: releaseNameStr,
        patchName: state.uploadName,
        patchDir: state.uploadPath,
        gameSourceDir: patchSourcePath || null,
        deletedFiles: fromCollect && patchResult ? patchResult.deleted_files : [],
        baseReleaseTag: fromCollect && patchResult ? patchResult.base_tag : null,
      });
      updateUploadState(releaseNameStr, (s) => { s.result = result; });
    } catch (e) {
      console.error("handleUploadPatch failed:", e);
      updateUploadState(releaseNameStr, (s) => { s.error = String(e); });
    } finally {
      updateUploadState(releaseNameStr, (s) => { s.uploading = false; });
      activeUploadVersion = null;
    }
  }

  async function handleCancelPatchUpload(event: Event, versionName: string) {
    event.stopPropagation();

    const state = ensureUploadState(versionName);
    if (!state.uploadName.trim()) return;

    await invoke<void>("cancel_patch_upload", { patchName: state.uploadName });
  }

  function syncExcludeTextToPatterns() {
    patchExcludePatterns = patchExcludeText
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l.length > 0);
    invoke("set_patch_exclude_patterns", { patterns: patchExcludePatterns });
  }

  function resetExcludePatterns() {
    patchExcludePatterns = [...DEFAULT_EXCLUDE_PATTERNS];
    patchExcludeText = patchExcludePatterns.join("\n");
    invoke("set_patch_exclude_patterns", { patterns: patchExcludePatterns });
  }

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

    try {
      await invoke<void>("create_release_repos", {
        name: $releaseName,
        path: $releasePath,
      });

      await startUploadingRelease();
    } catch (e) {
      console.error("handleCreateRelease failed:", e);
      showUploading.set(false);
    } finally {
      setTimeout(() => {
        inProcess.set(false);
      }, 2000);
    }
  }

  async function startUploadingRelease() {
    try {
      await invoke<void>("upload_v2_release", {
        name: $releaseName,
        path: $releasePath,
      });
    } catch (e) {
      console.error("startUploadingRelease failed:", e);
    }

    await fetchVersions();
  }

  async function chooseNewReleasePath(event: Event) {
    event.stopPropagation();

    await choosePath((selected) => releasePath.set(selected));
  }

  async function handleContinueUploading() {
    inProcess.set(true);

    // Read counts from config directly (get_upload_manifest was removed).
    try {
      const cfg = await invoke<AppConfig>("get_config");
      if (cfg.progress_upload) {
        totalFiles.set(cfg.progress_upload.total_files);
        uploadedFiles.set(cfg.progress_upload.uploaded_files.length);
      }
    } catch (e) {
      console.error("Failed to read upload progress:", e);
    }

    let uploadCompleted = false;
    try {
      await invoke<void>("continue_upload_v2", { name: $releaseName });
      uploadCompleted = true;
    } catch (e) {
      console.error("handleContinueUploading failed:", e);
    } finally {
      // Only hide the upload item if the upload actually finished (progress_upload cleared).
      // If resume failed, progress_upload is still in config and the UI item must stay visible.
      inProcess.set(false);
      expandedIndex = null;
      if (uploadCompleted) {
        setTimeout(() => {
          showUploading.set(false);
        }, 2000);
        await fetchVersions();
      }
    }
  }

  async function handleCancelUploading() {
    try {
      await invoke<void>("cancel_upload", { name: $releaseName });
    } catch (e) {
      console.error("handleCancelUploading failed:", e);
    }
  }

  function toggleExpand(index: number) {
    expandedIndex = expandedIndex === index ? null : index;
  }

  $effect(() => {
    // Re-check saved upload progress whenever providers init or the upload UI is hidden.
    // If progress_upload still exists in config (interrupted upload), restore the item.
    if ($providersWasInited && !$showUploading) {
      invoke<AppConfig>("get_config").then((config) => {
        // Guard: only restore if progress_upload is a real in-progress upload
        // (name non-empty and not completed). An empty {} object from an old
        // config or a manual reset must NOT be treated as an active upload.
        if (
          !!config.progress_upload &&
          !!config.progress_upload.name &&
          !config.progress_upload.is_completed
        ) {
          showUploading.set(true);
          releaseName.set(config.progress_upload.name);
          totalFiles.set(config.progress_upload.total_files);
          uploadedFiles.set(config.progress_upload.uploaded_files.length);
        }
      });
    }
  });

  // Load persisted patch settings from config on init.
  let patchConfigLoaded = false;
  $effect(() => {
    if ($providersWasInited && !patchConfigLoaded) {
      patchConfigLoaded = true;
      invoke<AppConfig>("get_config").then((config) => {
        if (config.patch_source_dir) {
          patchSourcePath = config.patch_source_dir;
        }
        if (config.patch_upload_dir) {
          lastPatchUploadPath = config.patch_upload_dir;
        }
        if (config.patch_exclude_patterns && config.patch_exclude_patterns.length > 0) {
          patchExcludePatterns = config.patch_exclude_patterns;
        }
        patchExcludeText = patchExcludePatterns.join("\n");
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

    <!-- Элемент сбора патча из git-репозиториев игры -->
    <div class="release-item patch-item">
      <div class="header" role="button" tabindex="0" onclick={() => toggleExpand(-3)}>
        <span class="plus-icon">±</span>
        <span class="placeholder-text">{$_("app.releases.patch.collectTitle")}</span>
      </div>
      {#if expandedIndex === -3}
        <div class="expanded-content">
          <div class="input-group">
            <label class="input-label">{$_("app.releases.patch.source")}</label>
            <div class="input-row">
              <input
                type="text"
                readonly
                bind:value={patchSourcePath}
                placeholder={$_("app.releases.patch.source")}
                class="release-input"
              />
              <button type="button" onclick={choosePatchSourcePath} class="choose-btn">
                {$_("app.releases.browse")}
              </button>
            </div>
          </div>
          <div class="input-group">
            <label class="input-label">{$_("app.releases.patch.excludePatterns")}</label>
            <textarea
              class="exclude-textarea"
              bind:value={patchExcludeText}
              placeholder={$_("app.releases.patch.excludePatterns")}
              rows="6"
            ></textarea>
            <button type="button" onclick={resetExcludePatterns} class="reset-btn">
              {$_("app.releases.patch.resetDefaults")}
            </button>
          </div>
          <button type="button" onclick={handleCollectPatch} class="create-btn" disabled={collectingPatch}>
            {#if collectingPatch}
              <Spin size={14} />
            {:else}
              {$_("app.releases.patch.collect")}
            {/if}
          </button>

          {#if patchError}
            <div class="patch-summary error-text">{patchError}</div>
          {/if}

          {#if patchResult}
            <div class="patch-summary">
              <span class="status-icon">✓</span>
              <span>{$_("app.releases.patch.done")}</span>
            </div>
            <div class="patch-report">
              <span>{$_("app.releases.patch.changed")}{patchResult.changed}</span>
              <span>{$_("app.releases.patch.deleted")}{patchResult.deleted}</span>
              <span class="patch-dir">{$_("app.releases.patch.patchDir")}{patchResult.patch_dir}</span>
            </div>
            <div class="patch-repos">
              <span class="patch-repos-title">{$_("app.releases.patch.reposReport")}</span>
              {#each patchResult.repos as repo}
                <div class="repo-row">
                  <span class="repo-path">{repo.repo_rel_path || "(root)"}</span>
                  <span class={repoStatusClass(repo.status)}>
                    {#if repo.status === "collected"}
                      {$_("app.releases.patch.statusCollected")}: {repo.changed}+/{repo.deleted}-
                    {:else if repo.status === "no_tags"}
                      {$_("app.releases.patch.statusNoTags")}
                    {:else if repo.status === "no_changes"}
                      {$_("app.releases.patch.statusNoChanges")}
                    {:else}
                      {$_("app.releases.patch.statusError")}{repo.message ? `: ${repo.message}` : ""}
                    {/if}
                  </span>
                </div>
              {/each}
            </div>
          {/if}
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
            <button type="button" onclick={handleCancelUploading} class="continue-btn" style="margin-right: 0.5rem;">
              {$_("app.releases.stop")}
            </button>
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
          {@const ups = readUploadState(version.name)}
          <div class="expanded-content installed-status">
            <span class="status-icon">✓</span>
            <span class="status-text">{$_("app.releases.installed")}</span>
          </div>
          <div class="expanded-content patch-upload-section" onclick={(e) => e.stopPropagation()}>
            <span class="patch-repos-title">{$_("app.releases.patch.addTitle")}</span>
            <div class="input-group">
              <label class="input-label">{$_("app.releases.patch.dir")}</label>
              <div class="input-row">
                <input
                  type="text"
                  readonly
                  value={ups.uploadPath}
                  placeholder={$_("app.releases.patch.dir")}
                  class="release-input"
                />
                <button type="button" onclick={(e) => choosePatchUploadPath(e, version.name)} class="choose-btn">
                  {$_("app.releases.browse")}
                </button>
              </div>
            </div>
            <div class="input-group">
              <label class="input-label">{$_("app.releases.patch.name")}</label>
              <div class="input-row">
                <input
                  type="text"
                  value={ups.uploadName}
                  oninput={(e) => updateUploadState(version.name, (s) => { s.uploadName = (e.target as HTMLInputElement).value; })}
                  placeholder={$_("app.releases.patch.name")}
                  class="release-input"
                />
              </div>
            </div>
            <div class="input-row patch-actions">
              <button type="button" onclick={(e) => handleUploadPatch(e, version.name)} class="create-btn" disabled={ups.uploading}>
                {#if ups.uploading}
                  <Spin size={14} />
                {:else}
                  {$_("app.releases.patch.add")}
                {/if}
              </button>
              {#if ups.uploading}
                <button type="button" onclick={(e) => handleCancelPatchUpload(e, version.name)} class="continue-btn">
                  {$_("app.releases.stop")}
                </button>
              {/if}
            </div>

            {#if ups.error}
              <div class="patch-summary error-text">{ups.error}</div>
            {/if}

            {#if ups.uploading || ups.files.size > 0}
              {#each [...ups.files] as [name, progress]}
                <div class="file-row">
                  <span>{name}</span>
                  <Progress height={12} maxWidth="1fr - 300px" progress={progress.progress} showPercents={false} />
                  <span style="justify-self: end;"
                    >{parseBytes(progress.file_uploaded_size)[0]}
                    {$_(`app.common.${parseBytes(progress.file_uploaded_size)[1]}`)} / {parseBytes(progress.file_total_size)[0]}
                    {$_(`app.common.${parseBytes(progress.file_total_size)[1]}`)}</span>
                </div>
              {/each}
            {/if}

            {#if ups.result}
              <div class="patch-summary">
                <span class="status-icon">✓</span>
                <span>{$_("app.releases.patch.uploaded")}</span>
              </div>
              {#if ups.result.warnings.length > 0}
                <div class="patch-repos">
                  {#each ups.result.warnings as w}
                    <span class="repo-status error">{w}</span>
                  {/each}
                </div>
              {/if}
              {#if ups.result.repos.length > 0}
                <div class="patch-repos">
                  <span class="patch-repos-title">{$_("app.releases.patch.tagReport")}</span>
                  {#each ups.result.repos as repo}
                    <div class="repo-row">
                      <span class="repo-path">{repo.repo_rel_path || "(root)"}</span>
                      <span class={repo.pushed ? "repo-status collected" : "repo-status error"}>
                        {#if repo.pushed}
                          {$_("app.releases.patch.tagOk")}
                        {:else}
                          {$_("app.releases.patch.tagFail")}{repo.message ? `: ${repo.message}` : ""}
                        {/if}
                      </span>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}

            {#each ups.log as text}
              <span class="log-text">{text}</span>
            {/each}
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

  .patch-summary {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 1rem;
    color: #4caf50;
    font-weight: 500;
  }

  .error-text {
    color: #f44336;
    font-weight: 500;
  }

  .patch-report {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-top: 0.5rem;
    color: #ddd;
    font-size: 0.9rem;
  }

  .patch-dir {
    color: #aaa;
    font-family: monospace;
    font-size: 0.8rem;
  }

  .patch-repos {
    margin-top: 1rem;
  }

  .patch-repos-title {
    display: block;
    margin-bottom: 0.5rem;
    color: #fff;
    font-weight: 500;
  }

  .repo-row {
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: 0.75rem;
    margin-bottom: 0.25rem;
    font-size: 0.85rem;
  }

  .repo-path {
    color: #aaa;
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-status.collected {
    color: #4caf50;
  }

  .repo-status.skipped {
    color: #999;
    font-style: italic;
  }

  .repo-status.error {
    color: #f44336;
  }

  .patch-upload-section {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .patch-actions {
    gap: 0.5rem;
  }

  .exclude-textarea {
    -webkit-app-region: no-drag;
    width: 100%;
    padding: 0.5rem;
    border: 1px solid #555;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.8);
    font-family: monospace;
    font-size: 0.8rem;
    resize: vertical;
    box-sizing: border-box;
  }
  .exclude-textarea:focus {
    background-color: rgba(255, 255, 255, 1);
    outline: none;
  }

  .reset-btn {
    -webkit-app-region: no-drag;
    margin-top: 0.4rem;
    padding: 0.3rem 0.8rem;
    color: #fff;
    background-color: rgba(120, 120, 120, 0.6);
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 0.8rem;
    transition: background-color 0.15s ease;
    align-self: flex-start;
  }
  .reset-btn:hover {
    background-color: rgba(120, 120, 120, 0.9);
  }
</style>
