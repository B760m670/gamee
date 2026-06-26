import {REPO} from './appVersion';

export interface LatestRelease {
  tag: string;
  name: string;
  notes: string;
  apkUrl: string | null;
  sizeBytes: number | null;
}

// Reads the latest published GitHub Release and its .apk asset.
// Public repo — no auth needed.
export async function fetchLatestRelease(): Promise<LatestRelease> {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: {Accept: 'application/vnd.github+json'},
  });
  if (!res.ok) {
    throw new Error(`GitHub API ${res.status}`);
  }
  const d: any = await res.json();
  const apk = (d.assets ?? []).find((a: any) => typeof a.name === 'string' && a.name.endsWith('.apk'));

  return {
    tag: d.tag_name ?? '',
    name: d.name ?? d.tag_name ?? '',
    notes: d.body ?? '',
    apkUrl: apk?.browser_download_url ?? null,
    sizeBytes: apk?.size ?? null,
  };
}
