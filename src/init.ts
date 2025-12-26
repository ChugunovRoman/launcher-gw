import { register, init as initLocales } from 'svelte-i18n';
import { Lang } from './consts';

import { initPackListener } from "./lib/pack";
import { initMainListeners } from "./lib/main";
import { initUnpackListener } from "./lib/unpack";
import { initUploadListeners } from "./lib/upload";
import { initDownloadListeners } from "./lib/download";

export async function init() {
  initMainListeners();
  initPackListener();
  initUnpackListener();
  initUploadListeners();
  initDownloadListeners();

  register(Lang.En, () => import('./locales/en.json'));
  register(Lang.Ru, () => import('./locales/ru.json'));

  return initLocales({
    initialLocale: Lang.Ru,
    fallbackLocale: Lang.Ru,
  });
}
