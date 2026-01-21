import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { applyKeyProfile, profileKeyMap, profiles, selectedProfile, updateCurrentBindsMap } from '../store/profiles';
import { appConfig } from '../store/main';
import { get } from 'svelte/store';
import { CUSTOM_BIND_LTX } from '../consts';

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

    if (cfg.selected_profile) {
      selectedProfile.set(cfg.selected_profile);
      applyKeyProfile.set(true);
    } else {
      selectedProfile.set(CUSTOM_BIND_LTX);
    }

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
