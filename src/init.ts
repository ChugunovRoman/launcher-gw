import { register, init as initLocales } from 'svelte-i18n';
import { invoke } from '@tauri-apps/api/core';
import { Lang } from './consts';

import { initPackListener } from "./lib/pack";
import { initMainListeners } from "./lib/main";
import { initUnpackListener } from "./lib/unpack";
import { initUploadListeners } from "./lib/upload";
import { initDownloadListeners } from "./lib/download";
import { initProfilesListeners } from './lib/profiles';
import { initPatchListeners } from './lib/patches';
import { initPatchUploadListeners } from './lib/patchUpload';

export async function init() {
  // Register listeners first (IPC subscriptions, millisecond-fast).
  await Promise.all([
    initProfilesListeners(),
    initMainListeners(),
    initPackListener(),
    initUnpackListener(),
    initUploadListeners(),
    initDownloadListeners(),
    initPatchListeners(),
    initPatchUploadListeners(),
  ]);

  register(Lang.En, () => import('./locales/en.json'));
  register(Lang.Ru, () => import('./locales/ru.json'));

  // Resolve the saved language BEFORE initializing locales so the first
  // frame renders in the correct language (no flicker from ru → en).
  const lang = await invoke<string>("get_lang").catch(() => Lang.Ru);
  // Map backend lang ("ru"/"en") to the Lang enum value.
  const locale = lang === "en" ? Lang.En : Lang.Ru;

  return initLocales({
    initialLocale: locale,
    fallbackLocale: Lang.Ru,
  });
}
