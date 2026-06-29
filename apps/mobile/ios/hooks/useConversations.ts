import { useCallback, useEffect, useState } from 'react'
import { type ConversationSummary, fetchConversations } from '../lib/chat'

const POLL_MS = 5000

export function useConversations() {
  const [items, setItems]     = useState<ConversationSummary[]>([])
  const [loading, setLoading] = useState(true)

  const reload = useCallback(async () => {
    try {
      setItems(await fetchConversations())
    } catch {
      /* keep previous list on transient errors */
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { reload() }, [reload])

  useEffect(() => {
    const id = setInterval(reload, POLL_MS)
    return () => clearInterval(id)
  }, [reload])

  return { items, loading, reload }
}
