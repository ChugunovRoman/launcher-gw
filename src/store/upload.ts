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
export const filesPerCommit = writable("10");
export const versions = createArrayStore<Version>();
export const logText = createArrayStore<string>();

export const totalFiles = createNumStore(0);
export const uploadedFiles = createNumStore(0);

export const mainVersion = writable<Version | undefined>();

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
