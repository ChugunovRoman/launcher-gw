import App from './App.svelte';
import { mount } from 'svelte';
import { langInit } from "./langInit";

langInit().then(() => {
  mount(App, {
    target: document.getElementById('app')!
  });
});
