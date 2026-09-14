import { writable } from 'svelte/store';

export const currentFile = writable("");
export const status = writable(0);
export const totalSize = writable(0);
export const processedSize = writable(0);
export const progress = writable(0);
export const isInProcess = writable(false);
export const finish = writable(false);
// Trigger for the finish animation: the Pack view resets it inside its effect
// right after starting the timers, so it cannot be used to show a message.
export const completed = writable(false);
// Sticky "the last pack finished successfully" flag (item 33): unlike
// `completed` it is reset only when a new pack starts, so the success message
// stays on screen.
export const success = writable(false);
// Text of the error that aborted the last pack ("" = no error).
export const error = writable("");
// Source files the packer had to skip as unreadable — the pack succeeded but
// is incomplete (item 33).
export const skippedFiles = writable<string[]>([]);
