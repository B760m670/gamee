import { useEffect, useRef, useState } from 'react'
import { api } from '../lib/api'

export interface PublicUser {
  id: string
  username: string | null
  display_name: string
  avatar_url: string | null
  bio: string | null
}

const DEBOUNCE_MS = 250
const MIN_CHARS    = 2

// Normalises a raw query the same way the server does: drop a leading @ and
// keep only username characters, so we can decide locally whether to search.
export function normalizeQuery(raw: string): string {
  return raw.trim().toLowerCase().replace(/^@/, '').replace(/[^a-z0-9_]/g, '')
}

export function useUserSearch(query: string) {
  const [results, setResults] = useState<PublicUser[]>([])
  const [loading, setLoading] = useState(false)
  const [error,   setError]   = useState<string | null>(null)

  const abortRef = useRef<AbortController | null>(null)

  useEffect(() => {
    const term = normalizeQuery(query)

    // Cancel any in-flight request when the query changes.
    abortRef.current?.abort()

    if (term.length < MIN_CHARS) {
      setResults([])
      setLoading(false)
      setError(null)
      return
    }

    setLoading(true)
    const controller = new AbortController()
    abortRef.current = controller

    const timer = setTimeout(async () => {
      try {
        const res = await api.get<{ data: PublicUser[] }>(
          `/api/v1/users/search?q=${encodeURIComponent(term)}`,
          controller.signal,
        )
        setResults(res.data ?? [])
        setError(null)
      } catch (err: unknown) {
        if (controller.signal.aborted) return // superseded — ignore
        setError(err instanceof Error ? err.message : 'Ошибка поиска')
        setResults([])
      } finally {
        if (!controller.signal.aborted) setLoading(false)
      }
    }, DEBOUNCE_MS)

    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [query])

  return { results, loading, error }
}
