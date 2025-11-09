import { listen } from '@tauri-apps/api/event';
import type { Event } from "@tauri-apps/api/event";
import { fontColor, connectStatus, providersWasInited } from '../store/main';
import { ConnectStatus } from "../consts";

const unlisten: Map<string, (() => void)> = new Map();

export async function initMainListeners() {
  unlisten.set('background-init-success', await listen('background-init-success', (event: Event<void>) => {
    console.log("background-init-success !");

    providersWasInited.set(true);
    connectStatus.set(ConnectStatus.Connnected);
    fontColor.set("rgba(69, 240, 97, 1)");
  }));
  unlisten.set('background-init-failed', await listen('background-init-failed', (event: Event<string>) => {
    console.log("background-init-failed !, error: ", event.payload);

    providersWasInited.set(false);
    connectStatus.set(ConnectStatus.ConnnectError);
    fontColor.set("rgba(254, 197, 208, 1)");
  }));
}
