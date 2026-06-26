import { api } from './api'
import type { PublicUser } from '../hooks/useUserSearch'

export type MessageStatus = 'sending' | 'sent' | 'failed'

export interface ChatMessage {
  id: string
  conversation_id: string
  sender_id: string
  content: string | null
  media_url: string | null
  client_id: string | null
  is_deleted: boolean
  created_at: string
  // Client-only optimistic state (not persisted server-side).
  status?: MessageStatus
}

export interface LastMessage {
  id: string
  sender_id: string
  content: string | null
  media_url: string | null
  created_at: string
}

export interface ConversationSummary {
  conversation_id: string
  other: PublicUser | null
  last_message: LastMessage | null
  unread: number
}

export interface OpenConversationResult {
  conversation_id: string
  other: PublicUser
}

export function openConversation(userId: string) {
  return api.post<{ data: OpenConversationResult }>('/api/v1/conversations/with', { user_id: userId })
    .then(r => r.data)
}

export function fetchConversations() {
  return api.get<{ data: ConversationSummary[] }>('/api/v1/conversations').then(r => r.data)
}

export function fetchMessages(conversationId: string, before?: string) {
  const q = before ? `?before=${encodeURIComponent(before)}` : ''
  return api.get<{ data: ChatMessage[]; other_last_read_at: string | null }>(
    `/api/v1/conversations/${conversationId}/messages${q}`,
  )
}

export function fetchNewMessages(conversationId: string, after: string) {
  return api.get<{ data: ChatMessage[]; other_last_read_at: string | null }>(
    `/api/v1/conversations/${conversationId}/messages?after=${encodeURIComponent(after)}`,
  )
}

export function postMessage(
  conversationId: string,
  payload: { client_id: string; content?: string; media_url?: string },
) {
  return api.post<{ data: ChatMessage }>(`/api/v1/conversations/${conversationId}/messages`, payload)
    .then(r => r.data)
}

export function markConversationRead(conversationId: string) {
  return api.post(`/api/v1/conversations/${conversationId}/read`, {})
}
