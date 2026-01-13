import { writable } from 'svelte/store';

export const currentFile = writable("");
export const status = writable(0);
export const totalSize = writable(0);
export const processedSize = writable(0);
export const progress = writable(0);
export const isInProcess = writable(false);
export const finish = writable(false);
export const completed = writable(false);
