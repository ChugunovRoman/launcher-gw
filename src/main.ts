import App from './App.svelte';
import { mount } from 'svelte';
import { init } from "./init";
import { bootstrap } from "./lib/bootstrap";

init()
  .catch((e) => {
    // Listener registration failures must not leave a blank white window.
    console.error("init() failed, mounting UI anyway:", e);
  })
  .finally(() => {
    mount(App, {
      target: document.getElementById('app')!
    });

    // Fill stores from backend commands (millisecond-fast, local I/O).
    // UI renders immediately from whatever is already in config; stores
    // are populated reactively after this call.
    bootstrap();
  });
