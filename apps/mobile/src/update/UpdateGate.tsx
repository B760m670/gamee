import React, {useState} from 'react';
import {Linking} from 'react-native';
import {useUpdate} from './useUpdate';
import {UpdateBanner} from './UpdateBanner';

// Shows the update banner when a newer release exists. For now "Обновить" opens
// the APK download (browser fallback); the next step replaces this with the
// in-app resumable downloader + MB progress.
export function UpdateGate() {
  const {available, latest} = useUpdate();
  const [dismissed, setDismissed] = useState(false);

  if (!available || dismissed || !latest?.apkUrl) {
    return null;
  }

  return (
    <UpdateBanner
      version={latest.tag}
      onUpdate={() => {
        if (latest.apkUrl) Linking.openURL(latest.apkUrl);
      }}
      onDismiss={() => setDismissed(true)}
    />
  );
}
