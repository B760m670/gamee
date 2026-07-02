const USERNAME_RE = /^[a-z0-9_]+$/

/** Returns a validation error message, or null if `username` is valid. Assumes already-trimmed/lowercased input. */
export function validateUsername(username: string): string | null {
  if (username.length < 5) return 'Минимум 5 символов'
  if (username.length > 32) return 'Максимум 32 символа'
  if (!USERNAME_RE.test(username)) return 'Только латинские буквы, цифры и «_»'
  return null
}
