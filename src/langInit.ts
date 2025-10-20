import { invoke } from '@tauri-apps/api/core';
import { register, init, locale } from 'svelte-i18n';
import { Lang } from './consts';

register(Lang.En, () => import('./locales/en.json'));
register(Lang.Ru, () => import('./locales/ru.json'));

init({
  fallbackLocale: Lang.Ru,
});

export const langInit = async () => {
  try {
    const lang = await invoke<string>('get_lang');
    const userLocale = lang || Lang.Ru;

    locale.set(userLocale);
  } catch (e) {
    console.error('Cannot get lang, error: ', e);
    locale.set(Lang.Ru);
  }
};
