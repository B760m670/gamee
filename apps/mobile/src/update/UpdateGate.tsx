import React, {useState} from 'react';
import {Linking} from 'react-native';
import {useUpdate} from './useUpdate';
import {useUpdateDownload} from './useUpdateDownload';
import {UpdateBanner} from './UpdateBanner';
import {UpdateProgress} from './UpdateProgress';

// Orchestrates the in-app update UX:
//   newer release found -> banner -> tap "Обновить" -> background download with
//   live MB progress -> auto-install on completion.
// If the native downloader isn't available, falls back to opening the APK URL.
export function UpdateGate() {
  const {available, latest} = useUpdate();
  const dl = useUpdateDownload();
  const [dismissed, setDismissed] = useState(false);

  if (!available || !latest?.apkUrl) {
    return null;
  }

  // Active download — show live progress instead of the banner.
  if (dl.status === 'downloading' || dl.status === 'paused' || dl.status === 'error') {
    return (
      <UpdateProgress
        bytes={dl.bytes}
        total={dl.total}
        paused={dl.status === 'paused'}
        error={dl.status === 'error'}
        onCancel={dl.cancel}
      />
    );
  }

  if (dismissed) {
    return null;
  }

  return (
    <UpdateBanner
      version={latest.tag}
      onUpdate={() => {
        if (dl.supported) {
          dl.start(latest.apkUrl as string);
        } else {
          Linking.openURL(latest.apkUrl as string);
        }
      }}
      onDismiss={() => setDismissed(true)}
    />
  );
}
