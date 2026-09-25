import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { get } from 'svelte/store';
import {
  patchUploads,
  patchCollectProgress,
  updatePatchUpload,
  resetPatchUploadProgress,
  isPatchUploadActive,
  PATCH_LOG_LIMIT,
  type PatchUploadState,
} from '../store/patchUpload';
import { parseBytes } from '../utils/dwn';

// Global listeners of the patch upload / collect events.
// Event contract: plans/launcher/patch-upload-status-plan.md, stage 1.

/// Fixed stage order of `upload_patch`.
export const PATCH_UPLOAD_STAGES: PatchUploadStage[] = [
  "prepare",
  "fe_fragment",
  "save_break_scan",
  "packing",
  "create_release",
  "upload",
  "publish_index",
  "tag_repos",
];

/// Localization keys of the stage names.
export const PATCH_STAGE_LABEL_KEYS: Record<string, string> = {
  prepare: "app.releases.patch.stage.prepare",
  fe_fragment: "app.releases.patch.stage.feFragment",
  save_break_scan: "app.releases.patch.stage.saveBreakScan",
  packing: "app.releases.patch.stage.packing",
  create_release: "app.releases.patch.stage.createRelease",
  upload: "app.releases.patch.stage.upload",
  publish_index: "app.releases.patch.stage.publishIndex",
  tag_repos: "app.releases.patch.stage.tagRepos",
};

/// Localization keys of the hints for the machine codes of the events.
export const PATCH_HINT_KEYS: Record<string, string> = {
  PATCH_UPLOAD_ALREADY_RUNNING: "app.releases.patch.hint.alreadyRunning",
  RELEASE_EXISTS: "app.releases.patch.hint.releaseExists",
  UPLOAD_HASH_MISMATCH: "app.releases.patch.hint.hashMismatch",
  UPDATES_REPO_NOT_FOUND: "app.releases.patch.hint.updatesRepoNotFound",
  INDEX_PUBLISH_FAILED: "app.releases.patch.hint.indexPublishFailed",
  SAVE_BREAKING: "app.releases.patch.hint.saveBreaking",
  PACK_SKIPPED_FILES: "app.releases.patch.hint.packSkippedFiles",
  TAG_PUSH_FAILED: "app.releases.patch.hint.tagPushFailed",
  NO_GAME_SOURCE_DIR: "app.releases.patch.hint.noGameSourceDir",
};

export const PATCH_CODE_ALREADY_RUNNING = "PATCH_UPLOAD_ALREADY_RUNNING";
export const PATCH_CODE_CANCELLED = "USER_CANCELLED";

/// Stage index in the fixed order (-1 for null / unknown).
export function patchStageIndex(stage: PatchUploadStage | null): number {
  return stage ? PATCH_UPLOAD_STAGES.indexOf(stage) : -1;
}

/// A duplicate start of a patch that is already uploading: its events must not
/// touch the state of the running upload.
function isForeignDuplicate(s: PatchUploadState | undefined, code: string | null): boolean {
  return !!s && code === PATCH_CODE_ALREADY_RUNNING && isPatchUploadActive(s) && !s.pendingStart;
}

function addWarning(s: PatchUploadState, message: string | null | undefined) {
  if (!message || s.warnings.includes(message)) return;
  s.warnings = [...s.warnings, message];
}

function handleStage(p: PatchUploadStagePayload) {
  const vn = p.release_name;
  const current = get(patchUploads).get(vn);
  const isStart = p.stage === "prepare" && p.state === "running";

  if (isForeignDuplicate(current, p.code)) return;
  // A repeated `prepare running` while this version is uploading and no new
  // start was requested belongs to a duplicate attempt — ignore it.
  if (isStart && current && isPatchUploadActive(current) && !current.pendingStart) return;

  updatePatchUpload(vn, (s) => {
    if (isStart) {
      resetPatchUploadProgress(s);
      s.status = "running";
    }
    s.patchTag = p.patch_tag;

    // A stage may get several warnings while it runs, and its final state is the
    // last event (a stage that had warnings ends with `warning` as well). An
    // intermediate warning cannot be told apart from the final one, so the step
    // simply shows the latest event; the status component keeps the spinner on
    // the current step until the next stage starts.
    if (p.state === "warning") addWarning(s, p.message);

    // The backend fails the stage with USER_CANCELLED right before
    // `finished.cancelled`: a cancelled step is shown as skipped, not failed.
    const state: StageState = p.state === "failed" && p.code === PATCH_CODE_CANCELLED ? "skipped" : p.state;

    s.stages = {
      ...s.stages,
      [p.stage]: {
        state,
        message: p.message ?? undefined,
        code: p.code ?? undefined,
      },
    };

    if (p.state === "running") {
      s.currentStage = p.stage;
    }
  });
}

function handleFinished(p: PatchUploadFinishedPayload) {
  const vn = p.release_name;
  const current = get(patchUploads).get(vn);

  if (isForeignDuplicate(current, p.code)) {
    // The running upload keeps its progress, only the error is shown.
    updatePatchUpload(vn, (s) => {
      s.actionError = p.message ?? p.code;
    });
    return;
  }

  updatePatchUpload(vn, (s) => {
    s.pendingStart = false;
    s.patchTag = p.patch_tag || s.patchTag;

    // Close the step that was still running when the command ended.
    const closeState: StageState = p.kind === "failed" ? "failed" : "skipped";
    const stages = { ...s.stages };
    for (const key of Object.keys(stages) as PatchUploadStage[]) {
      const info = stages[key];
      if (info && info.state === "running") {
        stages[key] = { ...info, state: p.kind === "done" ? "done" : closeState };
      }
    }
    if (p.kind === "failed" && p.stage) {
      const info = stages[p.stage];
      stages[p.stage] = {
        state: "failed",
        message: p.message ?? info?.message,
        code: p.code ?? info?.code,
      };
    }
    s.stages = stages;

    switch (p.kind) {
      case "done":
        s.status = "done";
        s.error = null;
        s.result = p.result;
        if (p.result) {
          if (p.result.repos.length > 0) s.repos = p.result.repos;
          for (const w of p.result.warnings) addWarning(s, w);
        }
        s.currentStage = null;
        // The form is ready for the next patch.
        s.uploadName = "";
        break;
      case "cancelled":
        s.status = "cancelled";
        s.error = null;
        break;
      default:
        s.status = "failed";
        s.error = {
          stage: p.stage,
          message: p.message ?? "",
          code: p.code,
        };
        break;
    }
  });

}

function handleManifest(p: PatchUploadManifestPayload) {
  updatePatchUpload(p.release_name, (s) => {
    const files = new Map(s.files);
    let total = 0;
    for (const file of p.files) {
      total += file.size;
      if (!files.has(file.name)) {
        files.set(file.name, {
          file_uploaded_size: 0,
          file_total_size: file.size,
          progress: 0,
          speedValue: 0,
          sfxValue: "",
        });
      }
    }
    s.files = files;
    s.filesTotal = p.files.length;
    if (s.totalSize === 0) s.totalSize = total;
  });
}

function handleRepoTagged(p: PatchRepoTaggedPayload) {
  updatePatchUpload(p.release_name, (s) => {
    const repos = s.repos.filter((r) => r.repo_rel_path !== p.report.repo_rel_path);
    s.repos = [...repos, p.report];
  });
}

function handleFileStatus(p: PatchFileStatusPayload) {
  updatePatchUpload(p.release_name, (s) => {
    s.fileStatus = new Map(s.fileStatus).set(p.file_name, p.status);
  });
}

function handleLog(p: PatchUploadLogPayload) {
  updatePatchUpload(p.release_name, (s) => {
    s.log = [...s.log.slice(-(PATCH_LOG_LIMIT - 1)), p.message];
  });
}

function handleFilesCount(p: PatchFilesCountPayload) {
  updatePatchUpload(p.release_name, (s) => {
    s.filesDone = p.done;
    s.filesTotal = p.total;
  });
}

function handleProgress(p: PatchUploadProgressPayload) {
  updatePatchUpload(p.release_name, (s) => {
    const [speedValue, sfxValue] = parseBytes(p.speed);
    const safeTotal = p.file_total_size > 0 ? p.file_total_size : 1;
    s.files = new Map(s.files).set(p.file_name, {
      file_uploaded_size: p.file_uploaded_size,
      file_total_size: p.file_total_size,
      progress: (p.file_uploaded_size / safeTotal) * 100,
      speedValue,
      sfxValue,
    });
    s.totalUploaded = p.total_uploaded_size;
    s.totalSize = p.total_size;
  });
}

function handlePackProgress(p: PatchPackProgressPayload) {
  const { patch_tag: _tag, release_name, ...pack } = p;
  updatePatchUpload(release_name, (s) => {
    s.pack = pack;
  });
}

const unlisten: Map<string, () => void> = new Map();

export async function initPatchUploadListeners() {
  if (unlisten.size > 0) return;

  unlisten.set('patch-upload-stage', await listen('patch-upload-stage', (e: Event<PatchUploadStagePayload>) => handleStage(e.payload)));
  unlisten.set('patch-upload-finished', await listen('patch-upload-finished', (e: Event<PatchUploadFinishedPayload>) => handleFinished(e.payload)));
  unlisten.set('patch-upload-manifest', await listen('patch-upload-manifest', (e: Event<PatchUploadManifestPayload>) => handleManifest(e.payload)));
  unlisten.set('patch-upload-repo-tagged', await listen('patch-upload-repo-tagged', (e: Event<PatchRepoTaggedPayload>) => handleRepoTagged(e.payload)));
  unlisten.set('patch-upload-file-status', await listen('patch-upload-file-status', (e: Event<PatchFileStatusPayload>) => handleFileStatus(e.payload)));
  unlisten.set('patch-upload-log', await listen('patch-upload-log', (e: Event<PatchUploadLogPayload>) => handleLog(e.payload)));
  unlisten.set('patch-upload-files-count', await listen('patch-upload-files-count', (e: Event<PatchFilesCountPayload>) => handleFilesCount(e.payload)));
  unlisten.set('patch-upload-progress', await listen('patch-upload-progress', (e: Event<PatchUploadProgressPayload>) => handleProgress(e.payload)));
  unlisten.set('patch-pack-progress', await listen('patch-pack-progress', (e: Event<PatchPackProgressPayload>) => handlePackProgress(e.payload)));
  unlisten.set('patch-collect-progress', await listen('patch-collect-progress', (e: Event<PatchCollectProgress>) => patchCollectProgress.set(e.payload)));
}
