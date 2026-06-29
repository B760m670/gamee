import { useCallback, useEffect, useRef, useState } from 'react'
import { randomUUID } from 'expo-crypto'
import { useAuthStore } from '../store/auth'
import {
  type ChatMessage, openConversation, fetchMessages, fetchNewMessages, postMessage, markConversationRead,
} from '../lib/chat'
import type { PublicUser } from './useUserSearch'

const PAGE = 30
const POLL_MS = 3000

function sortDesc(list: ChatMessage[]): ChatMessage[] {
  return [...list].sort((a, b) => b.created_at.localeCompare(a.created_at))
}

function mergeOne(prev: ChatMessage[], m: ChatMessage): ChatMessage[] {
  let replaced = false
  const next = prev.map(x => {
    if ((m.client_id && x.client_id === m.client_id) || x.id === m.id) {
      replaced = true
      return m
    }
    return x
  })
  if (!replaced) next.push(m)
  return sortDesc(next)
}

export function useChat(userId: string) {
  const me = useAuthStore(s => s.user?.id) ?? ''

  const [conversationId, setConversationId] = useState<string | null>(null)
  const [other, setOther]                   = useState<PublicUser | null>(null)
  const [messages, setMessages]             = useState<ChatMessage[]>([])
  const [loading, setLoading]               = useState(true)
  const [error, setError]                   = useState<string | null>(null)
  const [otherLastReadAt, setOtherLastReadAt] = useState<string | null>(null)
  const [hasMore, setHasMore]               = useState(false)
  const loadingOlder = useRef(false)
  // Tracks the server-side timestamp of the newest known message for polling.
  // Updated only from server responses to avoid local clock skew issues.
  const pollAfterRef = useRef<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    ;(async () => {
      try {
        const openedAt = new Date().toISOString()
        const { conversation_id, other } = await openConversation(userId)
        if (cancelled) return
        setConversationId(conversation_id)
        setOther(other)

        const res    = await fetchMessages(conversation_id)
        if (cancelled) return
        const sorted = sortDesc((res.data ?? []).map(m => ({ ...m, status: 'sent' as const })))
        setMessages(sorted)
        pollAfterRef.current = sorted[0]?.created_at ?? openedAt
        setOtherLastReadAt(res.other_last_read_at)
        setHasMore(sorted.length >= PAGE)
        markConversationRead(conversation_id).catch(() => {})
      } catch (e: unknown) {
        if (!cancelled) setError(e instanceof Error ? e.message : 'Не удалось открыть чат')
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => { cancelled = true }
  }, [userId])

  // Poll every POLL_MS for new messages from the other participant.
  useEffect(() => {
    if (!conversationId) return
    const tick = async () => {
      const after = pollAfterRef.current
      if (!after) return
      try {
        const res = await fetchNewMessages(conversationId, after)
        if (res.data.length > 0) {
          const newMsgs = res.data.map(m => ({ ...m, status: 'sent' as const }))
          const newest  = newMsgs.reduce((a, b) => a.created_at > b.created_at ? a : b)
          if (newest.created_at > after) pollAfterRef.current = newest.created_at
          setMessages(prev => {
            let next = [...prev]
            newMsgs.forEach(m => { next = mergeOne(next, m) })
            return next
          })
          markConversationRead(conversationId).catch(() => {})
        }
        if (res.other_last_read_at !== undefined) setOtherLastReadAt(res.other_last_read_at)
      } catch { /* ignore transient poll errors */ }
    }
    const id = setInterval(tick, POLL_MS)
    return () => clearInterval(id)
  }, [conversationId])

  const send = useCallback(async (text: string) => {
    const content = text.trim()
    if (!content || !conversationId) return
    const clientId = randomUUID()
    const optimistic: ChatMessage = {
      id: clientId, conversation_id: conversationId, sender_id: me,
      content, media_url: null, client_id: clientId, is_deleted: false,
      created_at: new Date().toISOString(), status: 'sending',
    }
    setMessages(prev => mergeOne(prev, optimistic))
    try {
      const saved = await postMessage(conversationId, { client_id: clientId, content })
      setMessages(prev => mergeOne(prev, { ...saved, status: 'sent' }))
    } catch {
      setMessages(prev => prev.map(x => x.client_id === clientId ? { ...x, status: 'failed' } : x))
    }
  }, [conversationId, me])

  const loadOlder = useCallback(async () => {
    if (!conversationId || loadingOlder.current || !hasMore) return
    loadingOlder.current = true
    try {
      const oldest = messages[messages.length - 1]?.created_at
      const res    = await fetchMessages(conversationId, oldest)
      const older  = (res.data ?? []).map(m => ({ ...m, status: 'sent' as const }))
      setMessages(prev => {
        const merged = [...prev]
        older.forEach(m => { if (!merged.some(x => x.id === m.id)) merged.push(m) })
        return sortDesc(merged)
      })
      setHasMore(older.length >= PAGE)
    } finally {
      loadingOlder.current = false
    }
  }, [conversationId, messages, hasMore])

  return { conversationId, other, messages, loading, error, otherLastReadAt, hasMore, me, send, loadOlder }
}
