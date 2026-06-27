import {NativeModules, NativeEventEmitter, Platform} from 'react-native';

// Bridge to the native UpdateModule (Android DownloadManager based).
const native = NativeModules.UpdateModule as
  | {
      startDownload(url: string): Promise<string>;
      cancel(): Promise<boolean>;
      install(): Promise<boolean>;
    }
  | undefined;

export const hasNativeUpdater = Platform.OS === 'android' && !!native;

const emitter = hasNativeUpdater ? new NativeEventEmitter(NativeModules.UpdateModule) : null;

export type UpdateStatus = 'downloading' | 'paused' | 'done' | 'error';

export interface UpdateProgressEvent {
  status: UpdateStatus;
  bytesDownloaded: number;
  totalBytes: number;
}

export function subscribeProgress(cb: (e: UpdateProgressEvent) => void) {
  return emitter?.addListener('UpdateProgress', cb);
}

export function startDownload(url: string): Promise<string> {
  if (!native) return Promise.reject(new Error('native updater unavailable'));
  return native.startDownload(url);
}

export function cancelDownload(): Promise<boolean> {
  if (!native) return Promise.resolve(false);
  return native.cancel();
}

export function installUpdate(): Promise<boolean> {
  if (!native) return Promise.resolve(false);
  return native.install();
}
