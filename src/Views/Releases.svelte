<!-- ReleasesView.svelte -->
<script lang="ts">
  import { _ } from "svelte-i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { join } from "@tauri-apps/api/path";
  import { configReady } from "../store/main";
  import { factionFieldKey } from "../lib/factionSettings";
  import { showUploading, inProcess, versions, logText, releaseName, releasePath, totalFiles, uploadedFiles, uploadFilesMap } from "../store/upload";
  import { loadVersions } from "../lib/versions";
  import { choosePath } from "../utils/path";
  import { DEFAULT_EXCLUDE_PATTERNS } from "../consts";

  import Progress from "../Components/Progress.svelte";
  import Button from "../Components/Button.svelte";
  import Spin from "../Components/Spin.svelte";
  import { getInMb, parseBytes, formatSpeedBytesPerSec } from "../utils/dwn";

  // 5.18: Use version name (or special string key) instead of numeric index
  // so that expanding a card survives a list replacement (e.g. after refresh).
  let expandedName = $state<string | null>(null);
  let refreshing = $state(false);
  // 5.20: surfaced to the user instead of console-only.
  let listError = $state("");
  let createError = $state("");
  // 5.21 / Q4: releases that exist on the provider but are missing from the
  // published index — i.e. not visible to players yet.
  let unpublished = $state<Set<string>>(new Set());
  // Two-step confirmation for the forced index re-publish (it can shrink the
  // index, so it must not fire on a single stray click).
  let forceConfirm = $state(false);
  let forcePublishing = $state(false);

  // REGR-4: Reactive guard — load versions when the tab opens AND config is
  // ready.  A plain onMount reads $configReady once; if the tab is opened
  // before init completes, the load never fires.
  let loaded = $state(false);
  $effect(() => {
    if ($configReady && !loaded) {
      loaded = true;
      fetchVersions();
    }
  });
  let republishingIndex = $state(false);
  let republishIndexMsg = $state<"ok" | "err" | "">("");
  let indexPreviewJson = $state("");
  let indexCommitting = $state(false);
  let indexCommitted = $state(false);

  // --- Get SHA (developer tool: server-side asset hashes, filled into a
  // full copy of the release/patch manifest.json for a straight paste-over) ---
  let shaBusy = $state<string | null>(null);
  let shaResults = $state<Map<string, ReleaseManifest>>(new Map());
  let shaErrors = $state<Map<string, string>>(new Map());

  async function handleGetSha(releaseNameStr: string) {
    if (shaBusy) return;
    shaBusy = releaseNameStr;
    // Svelte 5 does not proxy Map: reassigning the SAME reference after
    // .delete() is a no-op for reactivity — build a new Map instead, same as
    // the Map-reactivity fix in Versions.svelte.
    shaErrors = new Map(shaErrors);
    shaErrors.delete(releaseNameStr);

    try {
      // The same updated manifest JSON is also pushed to the log panel by
      // the backend via the upload-log listener.
      const result = await invoke<ReleaseManifest>("get_release_assets_sha", { name: releaseNameStr });
      shaResults = new Map(shaResults).set(releaseNameStr, result);
    } catch (e: any) {
      const msg = typeof e === "string" ? e : String(e?.message ?? e);
      shaErrors = new Map(shaErrors).set(releaseNameStr, msg);
      logText.push(msg);
    } finally {
      shaBusy = null;
    }
  }

  function shaResultJson(manifest: ReleaseManifest): string {
    return JSON.stringify(manifest, null, 2);
  }

  // --- Patch collection (stage 1 of partial updates) ---
  let patchSourcePath = $state("");
  let collectingPatch = $state(false);
  let patchResult = $state<PatchCollectResult | null>(null);
  let patchError = $state("");
  let patchExcludePatterns = $state<string[]>([...DEFAULT_EXCLUDE_PATTERNS]);
  let patchExcludeText = $state("");
  // Faction editor props the developer keeps in the patch fragment.  Filled
  // from every successful collect (all props enabled) and sent to upload_patch.
  // Svelte 5 does not proxy Set: every mutation must reassign a NEW Set,
  // same as the Map-reactivity pattern used above for shaResults/shaErrors.
  let feSelectedFields = $state<Set<string>>(new Set());

  function toggleFeField(field: string) {
    const next = new Set(feSelectedFields);
    if (next.has(field)) next.delete(field);
    else next.add(field);
    feSelectedFields = next;
  }

  /// Opens the folder holding the pending fragment in the system file manager
  /// (same command the version screen uses for the game/log folders).
  async function handleShowFeFragment(event: Event) {
    event.stopPropagation();

    if (!patchResult) return;

    try {
      const path = await join(patchResult.patch_dir, "appdata", "patches");
      await invoke("open_explorer", { path });
    } catch (e) {
      console.error("handleShowFeFragment failed:", e);
    }
  }

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
    feSelectedFields = new Set();

    try {
      patchResult = await invoke<PatchCollectResult>("collect_patch", {
        sourceDir: patchSourcePath,
        excludePatterns: patchExcludePatterns,
      });
      // Every collected prop is enabled by default — the developer only has to
      // uncheck what must not reach the players.
      feSelectedFields = new Set(patchResult.fe_updated_fields);
      // Persist the source path for next session.
      invoke("set_patch_source_dir", { source: patchSourcePath });
      // Prefill the per-version "add patch" form with the collected folder.
      lastPatchUploadPath = patchResult.patch_dir;
      // If a version is expanded, also update its state directly.
      if (expandedName !== null && !expandedName.startsWith("__")) {
        const vn = expandedName;
        if (vn)
          updateUploadState(vn, (s) => {
            s.uploadPath = patchResult!.patch_dir;
          });
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
      updateUploadState(versionName, (s) => {
        s.uploadPath = selected;
      });
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
        // Faction editor props kept by the developer; the backend rebuilds the
        // fragment from them (empty list = no fragment travels with the patch).
        updatedFields: fromCollect && patchResult ? patchResult.fe_updated_fields.filter((f) => feSelectedFields.has(f)) : null,
      });
      updateUploadState(releaseNameStr, (s) => {
        s.result = result;
      });
    } catch (e) {
      console.error("handleUploadPatch failed:", e);
      updateUploadState(releaseNameStr, (s) => {
        s.error = String(e);
      });
    } finally {
      updateUploadState(releaseNameStr, (s) => {
        s.uploading = false;
      });
      activeUploadVersion = null;
    }
  }

  async function handleCancelPatchUpload(event: Event, versionName: string) {
    event.stopPropagation();

    const state = ensureUploadState(versionName);
    if (!state.uploadName.trim()) return;

    await invoke<void>("cancel_patch_upload", { patchName: state.uploadName });
  }

  async function handlePreviewIndex() {
    republishingIndex = true;
    republishIndexMsg = "";
    indexPreviewJson = "";
    indexCommitted = false;
    try {
      indexPreviewJson = await invoke<string>("preview_index");
    } catch (e) {
      console.error("preview_index failed:", e);
      republishIndexMsg = "err";
    } finally {
      republishingIndex = false;
    }
  }

  async function handleCommitIndex() {
    if (!indexPreviewJson) return;
    indexCommitting = true;
    republishIndexMsg = "";
    try {
      await invoke<void>("commit_index", { json: indexPreviewJson });
      republishIndexMsg = "ok";
      indexCommitted = true;
    } catch (e) {
      console.error("commit_index failed:", e);
      republishIndexMsg = "err";
    } finally {
      indexCommitting = false;
    }
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
    try {
      // Forced: this dev-only view must also show releases that exist on the
      // provider but are not in the published index yet (freshly created, or
      // an upload that never finished).  The non-forced path is index-only,
      // so those releases would be invisible here and the "not published"
      // badge would have no row to mark.
      await loadVersions(true);
      listError = "";
    } catch (e) {
      console.error("fetchVersions failed:", e);
      listError = errText(e);
    }
    await fetchUnpublished();
  }

  async function handleRefresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      await loadVersions(true);
      listError = "";
    } catch (e) {
      console.error("handleRefresh failed:", e);
      listError = errText(e);
    } finally {
      refreshing = false;
    }
    await fetchUnpublished();
  }

  function errText(e: any): string {
    return typeof e === "string" ? e : String(e?.message ?? e);
  }

  /// Which releases are not in the published index yet (dev-only, needs a token).
  async function fetchUnpublished() {
    try {
      const names = await invoke<string[]>("get_unpublished_releases");
      unpublished = new Set(names);
    } catch (e) {
      console.error("get_unpublished_releases failed:", e);
      listError = errText(e);
    }
  }

  async function handleForceRepublish() {
    if (forcePublishing) return;
    if (!forceConfirm) {
      forceConfirm = true;
      return;
    }
    forcePublishing = true;
    republishIndexMsg = "";
    try {
      await invoke<void>("republish_index_force");
      republishIndexMsg = "ok";
      await fetchUnpublished();
    } catch (e) {
      console.error("republish_index_force failed:", e);
      republishIndexMsg = "err";
      listError = errText(e);
    } finally {
      forcePublishing = false;
      forceConfirm = false;
    }
  }

  async function handleCreateRelease(event: Event) {
    event.stopPropagation();

    if (!$releaseName.trim() || !$releasePath.trim()) return;

    showUploading.set(true);
    inProcess.set(true);
    expandedName = "__uploading__";

    console.log("handleCreateRelease, ", {
      newReleaseName: $releaseName,
      newReleasePath: $releasePath,
    });

    try {
      await invoke<void>("create_release_repos", {
        name: $releaseName,
        path: $releasePath,
      });

      // Stage 1 acceptance: the release must appear in the list right away,
      // not only after step_finalize republishes the index.
      await fetchVersions();

      await startUploadingRelease();
    } catch (e) {
      console.error("handleCreateRelease failed:", e);
      createError = errText(e);
      showUploading.set(false);
    } finally {
      setTimeout(() => {
        inProcess.set(false);
      }, 2000);
    }
  }

  async function startUploadingRelease() {
    let uploaded = false;
    try {
      await invoke<void>("upload_v2_release", {
        name: $releaseName,
        path: $releasePath,
      });
      uploaded = true;
      createError = "";
    } catch (e) {
      console.error("startUploadingRelease failed:", e);
      createError = errText(e);
    }

    await fetchVersions();

    if (uploaded) {
      // Same cleanup (and same 2s grace period) as the resume path: leaving
      // the form filled in makes the next "create release" click act on the
      // previous release's name, and the upload card must not linger next to
      // the list entry the release now has.
      setTimeout(() => {
        showUploading.set(false);
        releaseName.set("");
        releasePath.set("");
      }, 2000);
    }
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
      createError = errText(e);
    } finally {
      // Only hide the upload item if the upload actually finished (progress_upload cleared).
      // If resume failed, progress_upload is still in config and the UI item must stay visible.
      inProcess.set(false);
      expandedName = null;
      if (uploadCompleted) {
        setTimeout(() => {
          showUploading.set(false);
          // Clear the form state so it does not interfere with the next
          // release creation or the version list filter.
          releaseName.set("");
          releasePath.set("");
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
      createError = errText(e);
    }
  }

  function toggleExpand(name: string) {
    expandedName = expandedName === name ? null : name;
  }

  $effect(() => {
    // Re-check saved upload progress whenever providers init or the upload UI is hidden.
    // If progress_upload still exists in config (interrupted upload), restore the item.
    if ($configReady && !$showUploading) {
      invoke<AppConfig>("get_config").then((config) => {
        // Guard: only restore if progress_upload is a real in-progress upload
        // (name non-empty and not completed). An empty {} object from an old
        // config or a manual reset must NOT be treated as an active upload.
        if (!!config.progress_upload && !!config.progress_upload.name && !config.progress_upload.is_completed) {
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
    if ($configReady && !patchConfigLoaded) {
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
      <div class="header" role="button" tabindex="0" onclick={() => toggleExpand("__add__")}>
        <span class="plus-icon">+</span>
        <span class="placeholder-text">{$_("app.releases.add")}</span>
      </div>
      {#if expandedName === "__add__"}
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

    {#if createError}
      <div class="release-item">
        <div class="expanded-content">
          <span class="repo-status error">{createError}</span>
        </div>
      </div>
    {/if}

    <!-- Refresh button (dev-only, already gated by allowPackMod in MenuBar) -->
    <div class="release-item add-item">
      <div class="header" role="button" tabindex="0" onclick={handleRefresh}>
        {#if refreshing}
          <Spin size={16} />
          <span class="placeholder-text">{$_("app.releases.refreshing")}</span>
        {:else}
          <span class="placeholder-text">{$_("app.releases.refresh")}</span>
        {/if}
      </div>
      {#if listError}
        <div class="expanded-content">
          <span class="repo-status error">{$_("app.releases.refreshFailed")}: {listError}</span>
        </div>
      {/if}
    </div>

    <!-- Элемент сбора патча из git-репозиториев игры -->
    <div class="release-item patch-item">
      <div class="header" role="button" tabindex="0" onclick={() => toggleExpand("__republish__")}>
        <span class="plus-icon">↻</span>
        <span class="placeholder-text">{$_("app.releases.republishIndex")}</span>
      </div>
      {#if expandedName === "__republish__"}
        <div class="expanded-content" onclick={(e) => e.stopPropagation()}>
          <button type="button" class="create-btn" disabled={republishingIndex} onclick={handlePreviewIndex}>
            {#if republishingIndex}
              <Spin size={14} />
            {:else}
              {$_("app.releases.republishIndex")}
            {/if}
          </button>

          <!-- Forced publish: skips the "index must not shrink" safety check.
               Needed after a release was deleted, otherwise every automatic
               publish keeps failing.  Two clicks to confirm. -->
          <button
            type="button"
            class="create-btn"
            style="margin-top: 0.5rem;"
            disabled={forcePublishing}
            title={$_("app.releases.forceRepublishHint")}
            onclick={handleForceRepublish}>
            {#if forcePublishing}
              <Spin size={14} />
            {:else if forceConfirm}
              {$_("app.releases.forceRepublishConfirm")}
            {:else}
              {$_("app.releases.forceRepublish")}
            {/if}
          </button>

          {#if indexPreviewJson}
            <label class="input-label" style="margin-top: 0.75rem;">index.json</label>
            <textarea
              class="index-preview-textarea"
              rows="20"
              bind:value={indexPreviewJson}
            ></textarea>
            <div class="input-row" style="margin-top: 0.5rem;">
              <button type="button" class="create-btn" disabled={indexCommitting || indexCommitted} onclick={handleCommitIndex}>
                {#if indexCommitting}
                  <Spin size={14} />
                {:else if indexCommitted}
                  ✓
                {:else}
                  {$_("app.releases.commitIndex")}
                {/if}
              </button>
            </div>
          {/if}

          {#if republishIndexMsg}
            <div class="patch-summary" class:error-text={republishIndexMsg === "err"}>
              {#if republishIndexMsg === "ok"}
                <span class="status-icon">✓</span>
                {$_("app.releases.republishIndexOk")}
              {:else}
                {$_("app.releases.republishIndexErr")}
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="release-item patch-item">
      <div class="header" role="button" tabindex="0" onclick={() => toggleExpand("__collect__")}>
        <span class="plus-icon">±</span>
        <span class="placeholder-text">{$_("app.releases.patch.collectTitle")}</span>
      </div>
      {#if expandedName === "__collect__"}
        <div class="expanded-content">
          <div class="input-group">
            <label class="input-label">{$_("app.releases.patch.source")}</label>
            <div class="input-row">
              <input type="text" readonly bind:value={patchSourcePath} placeholder={$_("app.releases.patch.source")} class="release-input" />
              <button type="button" onclick={choosePatchSourcePath} class="choose-btn">
                {$_("app.releases.browse")}
              </button>
            </div>
          </div>
          <div class="input-group">
            <label class="input-label">{$_("app.releases.patch.excludePatterns")}</label>
            <textarea class="exclude-textarea" bind:value={patchExcludeText} placeholder={$_("app.releases.patch.excludePatterns")} rows="6"
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

            <!-- Save-break detector: the patch carries level/spawn files, so
                 players' old saves stop working after installing it. Shown
                 before upload so such a patch is never published unnoticed.
                 Capped at the first 20 markers — enough to see the cause. -->
            {#if patchResult.breaks_saves}
              <div class="patch-save-break">
                <span>{$_("app.patches.saveBreakDevWarning")}</span>
                <ul>
                  {#each patchResult.save_breaking_files.slice(0, 20) as file}
                    <li>{file}</li>
                  {/each}
                  {#if patchResult.save_breaking_files.length > 20}
                    <li>… +{patchResult.save_breaking_files.length - 20}</li>
                  {/if}
                </ul>
              </div>
            {/if}
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

            <!-- Faction editor settings carried by the patch: the developer
                 reviews the changed props and can drop any of them before
                 uploading. -->
            <div class="patch-repos">
              {#if patchResult.fe_updated_fields.length === 0}
                <span class="repo-status skipped">{$_("app.releases.fePatchNone")}</span>
              {:else}
                <span class="patch-repos-title">{$_("app.releases.fePatchTitle")}</span>
                <div class="patch-report">
                  <span
                    >{$_("app.releases.fePatchSummary", {
                      values: { fields: patchResult.fe_updated_fields.length, entries: patchResult.fe_fragment_entries },
                    })}</span>
                </div>
                <div class="fe-fields">
                  {#each patchResult.fe_updated_fields as field}
                    <label class="fe-field-row">
                      <input type="checkbox" checked={feSelectedFields.has(field)} onchange={() => toggleFeField(field)} />
                      <span>{$_(factionFieldKey(field), { default: field })}</span>
                    </label>
                  {/each}
                </div>
                <button type="button" onclick={handleShowFeFragment} class="reset-btn">
                  {$_("app.releases.fePatchShowFragment")}
                </button>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    {#if $showUploading}
      <div class="release-item uplaod-item" onclick={() => toggleExpand("__uploading__")}>
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
        {#if expandedName === "__uploading__"}
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
    <!-- Key on path+name: either alone can collide (GitLab subgroups may
         share a display name, GitHub descriptions may share a path), and a
         duplicate key throws and takes the whole view down. -->
    {#each $versions as version (version.path + '|' + version.name)}
      <div class="release-item" onclick={() => toggleExpand(version.name)}>
        <div class="header">
          <span class="version-name">{version.name}</span>
          {#if unpublished.has(version.name)}
            <span class="not-published-badge" title={$_("app.releases.notPublishedHint")}>
              {$_("app.releases.notPublished")}
            </span>
          {/if}
        </div>
        {#if expandedName === version.name}
          {@const ups = readUploadState(version.name)}
          <div class="expanded-content installed-status">
            <span class="status-icon">✓</span>
            <span class="status-text">{$_("app.releases.installed")}</span>
            <button
              type="button"
              class="get-sha-btn"
              disabled={shaBusy === version.name}
              title={$_("app.releases.getShaHint")}
              onclick={(e) => {
                e.stopPropagation();
                handleGetSha(version.name);
              }}>
              {#if shaBusy === version.name}
                <Spin size={14} />
              {:else}
                {$_("app.releases.getSha")}
              {/if}
            </button>
          </div>
          {#if shaErrors.has(version.name)}
            <div class="expanded-content"><span class="repo-status error">{shaErrors.get(version.name)}</span></div>
          {/if}
          {#if shaResults.has(version.name)}
            <div class="expanded-content" onclick={(e) => e.stopPropagation()}>
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="input-label" style="margin-bottom: 0.4rem;">{$_("app.releases.getShaResult")}</label>
              <textarea class="index-preview-textarea" rows="16" readonly value={shaResultJson(shaResults.get(version.name)!)}></textarea>
            </div>
          {/if}
          <div class="expanded-content patch-upload-section" onclick={(e) => e.stopPropagation()}>
            <span class="patch-repos-title">{$_("app.releases.patch.addTitle")}</span>
            <div class="input-group">
              <label class="input-label">{$_("app.releases.patch.dir")}</label>
              <div class="input-row">
                <input type="text" readonly value={ups.uploadPath} placeholder={$_("app.releases.patch.dir")} class="release-input" />
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
                  oninput={(e) =>
                    updateUploadState(version.name, (s) => {
                      s.uploadName = (e.target as HTMLInputElement).value;
                    })}
                  placeholder={$_("app.releases.patch.name")}
                  class="release-input" />
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

  .not-published-badge {
    margin-left: 0.5rem;
    padding: 1px 6px;
    font-size: 0.72rem;
    color: #ffca28;
    border: 1px solid rgba(255, 193, 7, 0.6);
    border-radius: 4px;
    white-space: nowrap;
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

  .get-sha-btn {
    -webkit-app-region: no-drag;
    margin-left: auto;
    padding: 0.3rem 1rem;
    color: #fff;
    background-color: rgba(61, 93, 236, 0.8);
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 0.8rem;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .get-sha-btn:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .get-sha-btn:disabled {
    opacity: 0.6;
    cursor: wait;
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

  /* Save-break warning in the collect report: red and loud — this is the one
     thing in the report the developer must not scroll past. */
  .patch-save-break {
    margin-top: 0.75rem;
    padding: 0.6rem 0.75rem;
    border: 1px solid #f44336;
    border-radius: 6px;
    color: #ff8a80;
    font-size: 0.85rem;
  }
  .patch-save-break ul {
    margin: 0.4rem 0 0 1.2rem;
    padding: 0;
    font-family: monospace;
    font-size: 0.8rem;
    color: #ddd;
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

  .fe-fields {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin-top: 0.5rem;
  }

  .fe-field-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: #ddd;
    font-size: 0.85rem;
    cursor: pointer;
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
  .index-preview-textarea {
    -webkit-app-region: no-drag;
    width: 100%;
    padding: 0.5rem;
    border: 1px solid #555;
    border-radius: 4px;
    background-color: rgba(255, 255, 255, 0.05);
    color: #ddd;
    font-family: monospace;
    font-size: 0.75rem;
    resize: vertical;
    box-sizing: border-box;
  }
  .index-preview-textarea::-webkit-scrollbar {
    width: 12px;
  }
  .index-preview-textarea::-webkit-scrollbar-track {
    background: transparent;
  }
  .index-preview-textarea::-webkit-scrollbar-thumb {
    background-color: rgba(61, 93, 236, 0.8);
    border-radius: 6px;
    border: 3px solid transparent;
    background-clip: content-box;
  }
  .index-preview-textarea::-webkit-scrollbar-thumb:hover {
    background-color: rgba(61, 93, 236, 1);
  }
  .index-preview-textarea::-webkit-scrollbar-button {
    display: none;
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
