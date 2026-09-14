import { get, writable } from 'svelte/store';
import { createArrayStore, createMapStore, createNumStore } from './helpers';

export const manifest = writable<ReleaseManifest | undefined>();
export const uploadFilesMap = createMapStore<String, UploadFileData>();

export const selectedVersion = writable<string | undefined>();
export const hasAnyLocalVersion = writable<boolean>(false);
export const showUploading = writable(false);
export const inProcess = writable(false);
export const releaseName = writable("");
export const releasePath = writable("");
export const versions = createArrayStore<Version>();
export const logText = createArrayStore<string>();

export const totalFiles = createNumStore(0);
export const uploadedFiles = createNumStore(0);

export const mainVersion = writable<Version | undefined>();

/// JS-only download progress fields that survive version list replacements
/// (e.g. when switching providers). Keyed by version name.
export const downloadStates = createMapStore<string, Version>();

export function updateVersionProgress(releaseName: string, cb: (data: Version) => Partial<Version>) {
  // Compute the patch once from the current versions store to avoid calling
  // `cb` twice (which resets aggregate byte counts mid-download).
  const current = get(versions).find(v => v.name === releaseName);
  // `downloadStates` exists precisely to survive version list replacements
  // (a provider switch drops the list mid-download), so it — and not a bare
  // `{ name }` stub — is the fallback base. Building the patch from a stub
  // zeroed the accumulated progress overlay.
  const saved = get(downloadStates).get(releaseName);
  const base = current ?? saved ?? ({ name: releaseName } as Version);
  const patch = cb(base);

  // Update in the main versions list if present.
  versions.update((data) =>
    data.map(v => v.name === releaseName ? { ...v, ...patch } : v)
  );
  // Mirror into the persistent progress overlay.
  downloadStates.update((map) => {
    const existing = map.get(releaseName) ?? saved;
    const updated = {
      ...(existing ?? { name: releaseName } as Version),
      ...patch,
    };
    const next = new Map(map);
    next.set(releaseName, updated);
    return next;
  });
}

export function removeDownloadState(releaseName: string) {
  downloadStates.delItem(releaseName);
}

export function restoreDownloadState(version: Version): Version {
  const saved = get(downloadStates).get(version.name);
  if (!saved) return version;
  // Only restore JS-only download fields (not Rust fields like id, path, etc.).
  return {
    ...version,
    inProgress: saved.inProgress || version.inProgress,
    isStoped: saved.isStoped || version.isStoped,
    wasCanceled: saved.wasCanceled || version.wasCanceled,
    downloadCurrentFile: saved.downloadCurrentFile || version.downloadCurrentFile,
    downloadProgress: saved.downloadProgress || version.downloadProgress,
    downloadedFilesCnt: saved.downloadedFilesCnt || version.downloadedFilesCnt,
    totalFileCount: saved.totalFileCount || version.totalFileCount,
    downloadedFileBytes: saved.downloadedFileBytes || version.downloadedFileBytes,
    downloadSpeed: saved.downloadSpeed || version.downloadSpeed,
    speedValue: saved.speedValue || version.speedValue,
    sfxValue: saved.sfxValue || version.sfxValue,
    status: saved.status || version.status,
    // The overlay entry can be created by an event handler that returns no
    // filesProgress at all (repair/progress events build it from `{name}`),
    // so this must not assume the Map exists.
    filesProgress: saved.filesProgress && saved.filesProgress.size > 0 ? saved.filesProgress : version.filesProgress,
    manifest: saved.manifest || version.manifest,
  };
}

export function updateVersion(releaseName: string, cb: (data: Version) => Partial<Version>) {
  versions.update((data) => {
    return data.map(version => {
      if (version.name == releaseName) {
        return {
          ...version,
          ...cb(version),
        };
      }
      return version;
    });
  });
}
export function removeVersion(releaseName: string) {
  versions.update((data) => {
    return data.filter(v => v.name !== releaseName);
  });
}
export function refreshVersions() {
  versions.set(get(versions));
}
export function updateEachVersion(cb: (data: Version) => Partial<Version>) {
  versions.update((data) => {
    return data.map(version => {
      return {
        ...version,
        ...cb(version),
      }
    });
  });
}
