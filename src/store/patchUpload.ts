import { get, writable } from 'svelte/store';
import { createMapStore } from './helpers';

// Patch upload state, kept in a global store (not in Releases.svelte) so it
// survives switching tabs. Filled by the listeners in lib/patchUpload.ts.
// plans/launcher/patch-upload-status-plan.md, stage 2.1.

export type PatchUploadStatus = "idle" | "running" | "cancelling" | "done" | "failed" | "cancelled";

export interface PatchStageInfo {
  state: StageState;
  message?: string;
  code?: string;
}

export interface PatchUploadError {
  stage: PatchUploadStage | null;
  message: string;
  code: string | null;
}

export interface PatchUploadState {
  uploadPath: string;
  uploadName: string;
  patchTag: string | null;
  status: PatchUploadStatus;
  /// Set by handleUploadPatch right before `invoke("upload_patch")` and cleared
  /// by the first `stage prepare running` event. Tells a fresh start apart from
  /// a duplicate attempt (PATCH_UPLOAD_ALREADY_RUNNING) that must not wipe the
  /// progress of the upload which is already running.
  pendingStart: boolean;
  stages: Partial<Record<PatchUploadStage, PatchStageInfo>>;
  currentStage: PatchUploadStage | null;
  pack: CompressProgressPayload | null;
  /// Per-file progress. `sfxValue` holds an `app.common.*Sfx` key for the speed unit.
  files: Map<string, UploadFileData>;
  fileStatus: Map<string, PatchFileStatus>;
  filesDone: number;
  filesTotal: number;
  totalUploaded: number;
  totalSize: number;
  repos: RepoTagReport[];
  warnings: string[];
  result: PatchUploadResult | null;
  error: PatchUploadError | null;
  /// Error of a user action that does not end the running upload: a failed
  /// `cancel_patch_upload` call or a duplicate start (PATCH_UPLOAD_ALREADY_RUNNING).
  actionError: string | null;
  log: string[];
}

export const PATCH_LOG_LIMIT = 200;

/// Upload state per version name (= `release_name` of the events).
export const patchUploads = createMapStore<string, PatchUploadState>();
/// Progress of `collect_patch` (null — nothing collected in this session yet).
export const patchCollectProgress = writable<PatchCollectProgress | null>(null);

export function createPatchUploadState(uploadPath = ""): PatchUploadState {
  return {
    uploadPath,
    uploadName: "",
    patchTag: null,
    status: "idle",
    pendingStart: false,
    stages: {},
    currentStage: null,
    pack: null,
    files: new Map(),
    fileStatus: new Map(),
    filesDone: 0,
    filesTotal: 0,
    totalUploaded: 0,
    totalSize: 0,
    repos: [],
    warnings: [],
    result: null,
    error: null,
    actionError: null,
    log: [],
  };
}

/// Drops the progress/result of the previous run, keeps the form fields.
export function resetPatchUploadProgress(s: PatchUploadState) {
  const fresh = createPatchUploadState(s.uploadPath);
  const { uploadPath, uploadName } = s;
  Object.assign(s, fresh, { uploadPath, uploadName });
}

export function isPatchUploadActive(s: PatchUploadState | undefined): boolean {
  return !!s && (s.status === "running" || s.status === "cancelling");
}

/// Non-mutating read for template use.
export function readPatchUpload(map: Map<string, PatchUploadState>, versionName: string, defaultPath = ""): PatchUploadState {
  return map.get(versionName) ?? createPatchUploadState(defaultPath);
}

/// Applies `updater` to a shallow copy of the version state (created when missing)
/// and publishes it with a new Map reference.
export function updatePatchUpload(versionName: string, updater: (s: PatchUploadState) => void, defaultPath = "") {
  patchUploads.update((map) => {
    const current = map.get(versionName);
    const next: PatchUploadState = current ? { ...current } : createPatchUploadState(defaultPath);
    updater(next);
    const nextMap = new Map(map);
    nextMap.set(versionName, next);
    return nextMap;
  });
}

export function getPatchUpload(versionName: string): PatchUploadState | undefined {
  return get(patchUploads).get(versionName);
}
