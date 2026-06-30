// Offline placeholder for the former cloud REST backend.
//
// The app used to talk to a Supabase Edge Function through this client. That
// backend has been removed: the messenger is moving to a pure peer-to-peer,
// end-to-end-encrypted core with no server at all. Until that native core
// lands, every request rejects with a clear "offline" error, so no code path
// reaches a server. Call sites already handle rejected promises, so the
// screens simply render as empty UI shells.

export class OfflineError extends Error {
  constructor(path: string) {
    super(`Offline: backend removed, P2P core not implemented yet (${path})`)
    this.name = 'OfflineError'
  }
}

class ApiClient {
  // Kept for source compatibility with the former token-based client.
  setToken(_token: string | null) {}

  get<T>(path: string, _signal?: AbortSignal): Promise<T> {
    return Promise.reject(new OfflineError(path))
  }

  post<T>(path: string, _body?: unknown): Promise<T> {
    return Promise.reject(new OfflineError(path))
  }

  put<T>(path: string, _body?: unknown): Promise<T> {
    return Promise.reject(new OfflineError(path))
  }

  delete<T>(path: string): Promise<T> {
    return Promise.reject(new OfflineError(path))
  }
}

export const api = new ApiClient()
