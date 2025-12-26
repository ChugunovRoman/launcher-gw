import { writable, type Writable, get } from 'svelte/store';

export function createMenuStore(init: string): Writable<string> & {
  select: (value: string) => void;
} {
  const { subscribe, set, update } = writable<string>(init);

  return {
    subscribe,
    set,
    update,
    select: (view: string) => {
      if (view !== get(currentView)) {
        previousView.set(get(currentView));
        currentView.set(view);
      }

      update(() => view);
    },
  };
}

export const currentView = createMenuStore("home");
export const previousView = writable("home");

