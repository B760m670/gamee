const API_URL = process.env.EXPO_PUBLIC_API_URL ?? 'https://vscasdbufemkzrjyqddn.supabase.co/functions/v1/dynamic-function'

class ApiClient {
  private token: string | null = null

  setToken(token: string | null) {
    this.token = token
  }

  private async request<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }
    if (this.token) {
      headers['Authorization'] = `Bearer ${this.token}`
    }

    const res = await fetch(`${API_URL}${path}`, {
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
      signal,
    })

    if (!res.ok) {
      const err = await res.json().catch(() => ({ message: 'Network error' }))
      throw new Error(err.message ?? 'Request failed')
    }

    return res.json()
  }

  get<T>(path: string, signal?: AbortSignal) {
    return this.request<T>('GET', path, undefined, signal)
  }

  post<T>(path: string, body?: unknown) {
    return this.request<T>('POST', path, body)
  }

  put<T>(path: string, body?: unknown) {
    return this.request<T>('PUT', path, body)
  }

  delete<T>(path: string) {
    return this.request<T>('DELETE', path)
  }
}

export const api = new ApiClient()
