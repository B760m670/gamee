import {useCallback, useEffect, useRef, useState} from 'react';
import {
  hasNativeUpdater,
  subscribeProgress,
  startDownload,
  cancelDownload,
  installUpdate,
  type UpdateStatus,
} from './nativeUpdater';

export interface DownloadState {
  supported: boolean;
  status: 'idle' | UpdateStatus;
  bytes: number;
  total: number;
  start: (url: string) => void;
  cancel: () => void;
}

// Drives the native background download and exposes live byte progress.
// The download continues if the app is minimized and resumes after network
// loss (handled by the system DownloadManager). On completion it auto-launches
// the installer.
export function useUpdateDownload(): DownloadState {
  const [status, setStatus] = useState<'idle' | UpdateStatus>('idle');
  const [bytes, setBytes] = useState(0);
  const [total, setTotal] = useState(0);
  const sub = useRef<{remove: () => void} | null | undefined>(null);

  useEffect(() => {
    if (!hasNativeUpdater) return;
    sub.current = subscribeProgress(e => {
      setBytes(e.bytesDownloaded);
      setTotal(e.totalBytes);
      setStatus(e.status);
      if (e.status === 'done') {
        installUpdate().catch(() => {});
      }
    });
    return () => sub.current?.remove();
  }, []);

  const start = useCallback((url: string) => {
    setStatus('downloading');
    setBytes(0);
    setTotal(0);
    startDownload(url).catch(() => setStatus('error'));
  }, []);

  const cancel = useCallback(() => {
    cancelDownload().catch(() => {});
    setStatus('idle');
    setBytes(0);
    setTotal(0);
  }, []);

  return {supported: hasNativeUpdater, status, bytes, total, start, cancel};
}
