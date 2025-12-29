export class BiMap<K, V> {
  private keyToValue = new Map<K, V>();
  private valueToKey = new Map<V, K>();

  constructor(entries?: readonly (readonly [K, V])[]) {
    if (entries) {
      for (const [key, value] of entries) {
        this.set(key, value);
      }
    }
  }

  public *[Symbol.iterator](): IterableIterator<[K, V]> {
    for (const entry of this.keyToValue.entries()) {
      yield entry;
    }
  }

  public set(key: K, value: V): void {
    // Удаляем старые связи, чтобы сохранить уникальность 1:1
    if (this.keyToValue.has(key)) {
      this.valueToKey.delete(this.keyToValue.get(key)!);
    }
    if (this.valueToKey.has(value)) {
      this.keyToValue.delete(this.valueToKey.get(value)!);
    }

    this.keyToValue.set(key, value);
    this.valueToKey.set(value, key);
  }

  public getValue(key: K): V | undefined {
    return this.keyToValue.get(key);
  }

  public getKey(value: V): K | undefined {
    return this.valueToKey.get(value);
  }
  public getKeys(): K[] {
    return [...this.keyToValue.keys()];
  }
  public getValues(): V[] {
    return [...this.keyToValue.values()];
  }

  public deleteByKey(key: K): void {
    const value = this.keyToValue.get(key);
    if (value !== undefined) {
      this.keyToValue.delete(key);
      this.valueToKey.delete(value);
    }
  }

  get size() {
    return this.keyToValue.size;
  }
}
