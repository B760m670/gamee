export const COOLDOWN_DAYS = 14

// Global namespace — will cover users, channels, bots in future
export const RESERVED_USERNAMES = new Set([
  'admin', 'administrator', 'support', 'help', 'bot', 'bots',
  'channel', 'channels', 'group', 'groups', 'system', 'official',
  'api', 'app', 'apps', 'dev', 'developer', 'developers',
  'root', 'moderator', 'mod', 'staff', 'team',
  'security', 'privacy', 'legal', 'terms', 'contact', 'abuse',
  'info', 'news', 'updates', 'notifications', 'alert', 'alerts',
  'me', 'you', 'user', 'users', 'profile', 'settings',
  'login', 'logout', 'register', 'signup', 'signin', 'auth',
  'home', 'feed', 'search', 'create', 'activity',
  'messages', 'message', 'chat', 'chats', 'call', 'calls',
  'ios', 'android', 'web', 'mobile', 'desktop',
  'about', 'faq', 'tour', 'explore', 'trending',
])

// Returns an error string or null if valid
export function validateUsername(raw: string): string | null {
  const u = raw.trim()
  if (u.length < 5) return 'Минимум 5 символов'
  if (u.length > 32) return 'Максимум 32 символа'
  if (!/^[a-zA-Z0-9_]+$/.test(u)) return 'Только латинские буквы, цифры и _'
  if (u.startsWith('_')) return 'Нельзя начинать с _'
  if (u.endsWith('_')) return 'Нельзя заканчивать на _'
  if (u.includes('__')) return 'Нельзя два _ подряд'
  if (RESERVED_USERNAMES.has(u.toLowerCase())) return 'Это имя зарезервировано'
  return null
}

// Returns how many days remain in the cooldown (0 = can change)
export function cooldownDaysLeft(changedAt: string | null): number {
  if (!changedAt) return 0
  const elapsedDays = (Date.now() - new Date(changedAt).getTime()) / 86_400_000
  return Math.max(0, Math.ceil(COOLDOWN_DAYS - elapsedDays))
}
