import { register, init, waitLocale } from 'svelte-i18n';
import { Lang } from './consts';

export async function initLang() {
  register(Lang.En, () => import('./locales/en.json'));
  register(Lang.Ru, () => import('./locales/ru.json'));

  init({
    initialLocale: Lang.Ru,
    fallbackLocale: Lang.Ru,
  });

  await waitLocale();
}
