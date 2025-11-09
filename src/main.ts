import App from './App.svelte';
import { mount } from 'svelte';
import { init } from "./init";

init().then(() => {
  mount(App, {
    target: document.getElementById('app')!
  });
});
