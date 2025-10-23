import App from './App.svelte';
import { mount } from 'svelte';
import { langInit } from "./langInit";
import { initPackListener } from "./lib/pack";
import { initUnpackListener } from "./lib/unpack";

initPackListener();
initUnpackListener();

langInit().then(() => {
  mount(App, {
    target: document.getElementById('app')!
  });
});
