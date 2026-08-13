import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { applyKeyProfile, profileKeyMap, profiles, selectedProfile, updateCurrentBindsMap } from '../store/profiles';
import { appConfig, updateConfig } from '../store/main';
import { get } from 'svelte/store';
import { DEFAULT_BIND_LTX } from '../consts';

const unlisten: Map<string, (() => void)> = new Map();

export function transformToKeymapArray(keybinds: Record<string, KeybindingMapData>): KeybindingMap[] {
  return Object.keys(keybinds).map((action) => {
    const binds = keybinds[action];
    return {
      action: action,
      key: binds.key,
      altkey: binds.altkey
    };
  });
}

export async function persistProfileSelection(profileName: string, apply: boolean) {
  updateConfig("selected_profile", profileName);
  updateConfig("apply_key_profile", apply);
  await invoke<void>("set_apply_profile", { profileName, apply });
}

export async function initProfilesListeners() {
  unlisten.set('load-key-profiles', await listen('load-key-profiles', (event: Event<ProfileItem[]>) => {
    for (const profile of event.payload) {
      profileKeyMap.setItem(profile.name, transformToKeymapArray(profile.keybinds));
      profiles.push({
        label: profile.name.replace(".ltx", ""),
        value: profile.name,
      });
      sortOptions();
    }

    const cfg = get(appConfig);
    const names = new Set(event.payload.map((p) => p.name));
    let name = cfg.selected_profile;
    if (!name || !names.has(name)) {
      name = names.has(DEFAULT_BIND_LTX) ? DEFAULT_BIND_LTX : event.payload[0]?.name;
    }

    const apply = cfg.apply_key_profile ?? !!cfg.selected_profile;
    if (name) {
      selectedProfile.set(name);
      if (name !== cfg.selected_profile) {
        persistProfileSelection(name, apply);
      }
    }
    applyKeyProfile.set(apply);

    updateCurrentBindsMap();
  }));
}

export function sortOptions() {
  profiles.set(get(profiles).sort((a, b) => {
    const isALatin = /^[A-Za-z]/.test(a.label);
    const isBLatin = /^[A-Za-z]/.test(b.label);

    if (isALatin && !isBLatin) return -1;
    if (!isALatin && isBLatin) return 1;

    return a.label.localeCompare(b.label, 'ru', { sensitivity: 'accent' });
  }));
}
