import { writable, type Writable } from 'svelte/store';

function createArrayStore<T>(): Writable<T[]> & {
  clear: () => void;
  push: (item: T) => void;
  replaceLast: (item: T) => void;
} {
  const { subscribe, set, update } = writable<T[]>([]);

  return {
    subscribe,
    set,
    update,
    clear: () => set([]),
    push: (item: T) => update(arr => [...arr, item]),
    replaceLast: (item: T) =>
      update(arr => {
        if (arr.length === 0) return [item]; // или можно оставить как [] — зависит от логики
        const newArr = [...arr];
        newArr[newArr.length - 1] = item;
        return newArr;
      })
  };
}

function createNumStore(init: number): Writable<number> & {
  add: (value: number) => void;
} {
  const { subscribe, set, update } = writable<number>(init);

  return {
    subscribe,
    set,
    update,
    add: (value: number) => update(current => (current + value)),
  };
}

export const showUploading = writable(false);
export const inProcess = writable(false);
export const releaseName = writable("");
export const releasePath = writable("");
export const filesPerCommit = writable("10");
export const versions = createArrayStore<Version>();
export const logText = createArrayStore<string>();

export const totalFiles = createNumStore(0);
export const uploadedFiles = createNumStore(0);
