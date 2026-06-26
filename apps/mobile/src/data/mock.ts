// Temporary local data layer that replaces the old Supabase/cloud backend.
// This will be swapped for the native P2P core later — the UI talks only to
// these shapes, so the screens don't need to change when the real core lands.

export interface PublicUser {
  id: string;
  username: string | null;
  display_name: string | null;
  avatar_url: string | null;
}

export interface LastMessage {
  content: string | null;
  media_url: string | null;
  sender_id: string;
  created_at: string;
}

export interface ConversationSummary {
  conversation_id: string;
  other: PublicUser | null;
  last_message: LastMessage | null;
  unread: number;
}

export const ME_ID = 'me';

const now = Date.now();
const iso = (minutesAgo: number) => new Date(now - minutesAgo * 60000).toISOString();

export const MOCK_USERS: PublicUser[] = [
  {id: 'u1', username: 'alice', display_name: 'Alice', avatar_url: null},
  {id: 'u2', username: 'bob', display_name: 'Bob', avatar_url: null},
  {id: 'u3', username: 'carol', display_name: 'Carol', avatar_url: null},
];

export const MOCK_CONVERSATIONS: ConversationSummary[] = [
  {
    conversation_id: 'c1',
    other: MOCK_USERS[0],
    last_message: {content: 'Привет! Как дела?', media_url: null, sender_id: 'u1', created_at: iso(5)},
    unread: 2,
  },
  {
    conversation_id: 'c2',
    other: MOCK_USERS[1],
    last_message: {content: 'До встречи', media_url: null, sender_id: ME_ID, created_at: iso(140)},
    unread: 0,
  },
  {
    conversation_id: 'c3',
    other: MOCK_USERS[2],
    last_message: null,
    unread: 0,
  },
];

export function normalizeQuery(q: string): string {
  return q.trim().replace(/^@/, '').toLowerCase();
}

// Lightweight stand-ins for the old data hooks.
export function useConversations() {
  return {items: MOCK_CONVERSATIONS, loading: false, reload: () => {}};
}

export function useUserSearch(query: string) {
  const term = normalizeQuery(query);
  const results =
    term.length < 2
      ? []
      : MOCK_USERS.filter(
          u =>
            (u.username ?? '').toLowerCase().includes(term) ||
            (u.display_name ?? '').toLowerCase().includes(term),
        );
  return {results, loading: false, error: null as string | null};
}
