import {useCallback, useEffect, useState} from 'react';
import {APP_VERSION} from './appVersion';
import {compareVersions} from './compareVersions';
import {fetchLatestRelease, type LatestRelease} from './fetchLatestRelease';

export interface UpdateState {
  latest: LatestRelease | null;
  available: boolean;
  checking: boolean;
  error: string | null;
  check: () => Promise<void>;
}

// Checks GitHub Releases for a newer version than this build.
// (The actual in-app download is added in the next step as a native service.)
export function useUpdate(): UpdateState {
  const [latest, setLatest] = useState<LatestRelease | null>(null);
  const [available, setAvailable] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const check = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      const release = await fetchLatestRelease();
      setLatest(release);
      const newer = compareVersions(release.tag, APP_VERSION) > 0;
      setAvailable(newer && !!release.apkUrl);
    } catch (e: any) {
      setError(e?.message ?? 'Не удалось проверить обновления');
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    check();
  }, [check]);

  return {latest, available, checking, error, check};
}
