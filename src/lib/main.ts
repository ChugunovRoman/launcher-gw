import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { fontColor, connectStatus, providersWasInited } from '../store/main';

const unlisten: Map<string, (() => void)> = new Map();

export async function initMainListeners() {
  unlisten.set('background-init-success', await listen('background-init-success', (event: Event<void>) => {
    console.log("background-init-success !");

    providersWasInited.set(true);
    connectStatus.set("connnected");
    fontColor.set("rgba(69, 240, 97, 1)");
  }));
  unlisten.set('background-init-failed', await listen('background-init-failed', (event: Event<string>) => {
    console.log("background-init-failed !, error: ", event.payload);

    providersWasInited.set(false);
    connectStatus.set("connnect_error");
    fontColor.set("rgba(254, 197, 208, 1)");
  }));
}
