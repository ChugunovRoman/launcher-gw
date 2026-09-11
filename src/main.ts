import App from './App.svelte';
import { mount } from 'svelte';
import { init } from "./init";
import { bootstrap } from "./lib/bootstrap";

init().then(() => {
  mount(App, {
    target: document.getElementById('app')!
  });

  // Fill stores from backend commands (millisecond-fast, local I/O).
  // UI renders immediately from whatever is already in config; stores
  // are populated reactively after this call.
  bootstrap();
});
