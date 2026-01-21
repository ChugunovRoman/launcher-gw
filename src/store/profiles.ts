import { get, writable } from 'svelte/store';
import { createArrayStore, createMapStore } from './helpers';
import { keybindsGroups, NO_KEY } from '../consts';

export const selectedProfileMap = writable<Map<string, { [action: string]: [string, string] }>>(keybindsGroups);
export const selectedProfile = writable<string | undefined>();
export const applyKeyProfile = writable<boolean>(false);
export const profiles = createArrayStore<Option>();
export const profileKeyMap = createMapStore<string, KeybindingMap[]>();

export const removeProfileName = writable<string | undefined>();
export const showDlgRemoveProfile = writable(false);
export const showDlgApplyProfile = writable(false);
export const showDlgApplyProfileOk = writable(false);

export function updateCurrentBindsMap() {
  const currentProfile = get(profileKeyMap).get(get(selectedProfile)!);
  if (currentProfile) {
    const updatedMap = new Map();
    // const foundBinds = [];
    // const notFoundBinds = [];

    for (const [groupName, props] of get(selectedProfileMap)) {
      const group: { [action: string]: [string, string] } = { ...props };
      for (const action of Object.keys(props)) {
        const exist = currentProfile.find((c) => c.action === action);
        if (exist) {
          // foundBinds.push(action);
          group[action] = [exist.key || NO_KEY, exist.altkey || NO_KEY];
        }
      }
      updatedMap.set(groupName, group);
    }

    // for (const item of currentProfile) {
    //   if (!foundBinds.includes(item.action)) {
    //     notFoundBinds.push(item.action);
    //   }
    // }

    // console.log(`updateCurrentBindsMap, notFoundBinds: `, notFoundBinds);

    selectedProfileMap.set(updatedMap);
  }
}
