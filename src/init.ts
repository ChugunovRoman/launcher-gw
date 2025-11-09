import { register, init as initLocales, waitLocale } from 'svelte-i18n';
import { Lang } from './consts';

import { initPackListener } from "./lib/pack";
import { initMainListeners } from "./lib/main";
import { initUnpackListener } from "./lib/unpack";
import { initUploadListeners } from "./lib/upload";

export async function init() {
  initMainListeners();
  initPackListener();
  initUnpackListener();
  initUploadListeners();

  register(Lang.En, () => import('./locales/en.json'));
  register(Lang.Ru, () => import('./locales/ru.json'));

  return initLocales({
    initialLocale: Lang.Ru,
    fallbackLocale: Lang.Ru,
  });
}
