import { useEffect, useRef, useState } from 'react'
import { queryLedgerUsernameOwner } from '../modules/spiritchat-crypto-core'

export interface FoundUser {
  peerId: string
  fingerprint: string
  publicKeyBase64: string
  username: string
}

export function normalizeUsernameQuery(raw: string): string {
  return raw.trim().toLowerCase().replace(/^@/, '')
}

/**
 * @username search is an exact lookup against this node's own materialized
 * `@username` ledger state, not a directory listing — there is no server
 * anywhere in this project that could hold a searchable index of every
 * handle, so unlike a typical "people search" this only ever answers "does
 * this exact handle currently resolve to someone on the chain" (0 or 1
 * result), not a live-narrowing list. See `queryLedgerUsernameOwner`'s own
 * doc comment for why this can occasionally miss a name someone else holds
 * (this node's local chain view not being caught up yet).
 */
export function useUsernameSearch(query: string) {
  const [results, setResults] = useState<FoundUser[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError]     = useState<string | null>(null)
  const token = useRef(0)

  useEffect(() => {
    const normalized = normalizeUsernameQuery(query)

    if (normalized.length < 5 || !/^[a-z0-9_]+$/.test(normalized)) {
      setResults([])
      setLoading(false)
      setError(null)
      return
    }

    const myToken = ++token.current
    setLoading(true)
    setError(null)

    const timer = setTimeout(async () => {
      try {
        const owner = await queryLedgerUsernameOwner(normalized)
        if (token.current !== myToken) return
        setResults(
          owner.status === 'found'
            ? [{ peerId: owner.peerId, fingerprint: owner.fingerprint, publicKeyBase64: owner.ownerPublicKeyBase64, username: normalized }]
            : []
        )
      } catch {
        if (token.current !== myToken) return
        setError('Не удалось выполнить поиск — проверь соединение')
        setResults([])
      } finally {
        if (token.current === myToken) setLoading(false)
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [query])

  return { results, loading, error }
}
