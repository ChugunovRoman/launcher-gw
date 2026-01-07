import { writable, type Writable } from 'svelte/store';

export function createArrayStore<T>(): Writable<T[]> & {
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

export function createMapStore<K, V>(): Writable<Map<K, V>> & {
  clear: () => void;
  setItem: (key: K, item: V) => void;
  delItem: (key: K) => void;
} {
  const { subscribe, set, update } = writable<Map<K, V>>(new Map());

  return {
    subscribe,
    set,
    update,
    clear: () => set(new Map()),
    setItem: (key: K, item: V) => update(map => { map.set(key, item); return map; }),
    delItem: (key: K) => update(map => { map.delete(key); return map; }),
  };
}

export function createNumStore(init: number): Writable<number> & {
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
