import { writable } from 'svelte/store';

export const connectStatus = writable("connnecting");
export const fontColor = writable("rgba(243, 240, 63, 1)");

export const providersWasInited = writable(false);

export const loadedTokens = writable(false);
export const tokens = writable<Map<string, string>>(new Map());
