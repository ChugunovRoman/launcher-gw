// src/lib/utils/error-queue.ts
import { invoke } from '@tauri-apps/api/core';
import { providersWasInited } from './store/main';

const queue: string[] = [];
let isInitialized = false;
let hasFlushed = false;

// Подписываемся на изменения инициализации
const unsubscribe = providersWasInited.subscribe((value: boolean) => {
  isInitialized = value;
  if (isInitialized && !hasFlushed && queue.length > 0) {
    flushQueue();
  }
});

function sendToRust(msg: string) {
  invoke('log_error', { msg }).catch(err => {
    console.error('Failed to send error to Rust:', err);
  });
}

function flushQueue() {
  console.debug(`Flushing ${queue.length} queued errors`);
  for (const msg of queue) {
    sendToRust(msg);
  }
  queue.length = 0;
  hasFlushed = true;
}

export function handleError(error: Error | string | Event) {
  let msg: string;
  if (error instanceof ErrorEvent) {
    msg = error.error?.stack || error.message || 'Unknown error';
  } else if (error instanceof PromiseRejectionEvent) {
    msg = error.reason?.stack || String(error.reason) || 'Unknown rejection';
  } else if (error instanceof Error) {
    msg = error.stack || error.message || 'Unknown error';
  } else {
    msg = String(error);
  }

  if (isInitialized) {
    sendToRust(msg);
  } else {
    queue.push(msg);
  }
}

// Глобальные обработчики — можно установить отсюда или извне
export function setupGlobalErrorHandlers() {
  window.addEventListener('error', handleError);
  window.addEventListener('unhandledrejection', handleError);
}

// Экспортируем для отмены подписки (опционально, например, при закрытии приложения)
export { unsubscribe as unsubscribeErrorQueue };
