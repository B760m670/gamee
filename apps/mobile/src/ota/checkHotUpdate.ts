import {Platform} from 'react-native';
import hotUpdate from 'react-native-ota-hot-update';
import ReactNativeBlobUtil from 'react-native-blob-util';

// Dev OTA channel: every push publishes a fresh JS bundle to this release.
// The app downloads and applies it on launch — no APK reinstall needed.
// (Native changes still require a new APK.)
const UPDATE_JSON =
  'https://github.com/B760m670/gamee/releases/download/ota/update.json';

interface OtaCallbacks {
  onProgress?: (percent: number) => void;
  onUpToDate?: () => void;
}

export async function checkHotUpdate(cb: OtaCallbacks = {}): Promise<void> {
  // OTA only applies to release builds (debug runs from Metro).
  if (Platform.OS !== 'android' || __DEV__) {
    return;
  }
  try {
    const current = Number(await Promise.resolve(hotUpdate.getCurrentVersion())) || 0;

    const res = await fetch(`${UPDATE_JSON}?t=${Date.now()}`);
    if (!res.ok) return;
    const info = await res.json();

    if (typeof info.version !== 'number' || info.version <= current || !info.downloadAndroidUrl) {
      cb.onUpToDate?.();
      return;
    }

    hotUpdate.downloadBundleUri(ReactNativeBlobUtil, info.downloadAndroidUrl, info.version, {
      updateSuccess: () => {},
      updateFail: () => {},
      restartAfterInstall: true,
      restartDelay: 300,
      progress: (written: number, total: number) => {
        if (total > 0) cb.onProgress?.((written / total) * 100);
      },
    });
  } catch {
    // Network/parse errors are non-fatal — just keep the current bundle.
  }
}
