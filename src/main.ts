import App from './App.svelte';
import { mount } from 'svelte';
import { initLang } from "./langInit";
import { initPackListener } from "./lib/pack";
import { initMainListeners } from "./lib/main";
import { initUnpackListener } from "./lib/unpack";
import { initUploadListeners } from "./lib/upload";

initMainListeners();
initPackListener();
initUnpackListener();
initUploadListeners();

initLang().then(() => {
  mount(App, {
    target: document.getElementById('app')!
  });
});
