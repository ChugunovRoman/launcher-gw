import { writable } from 'svelte/store';

export const progress = writable(0);
export const isInProcess = writable(false);
export const finish = writable(false);
export const completed = writable(false);
