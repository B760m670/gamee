import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import { api } from '../lib/api'

const ACCOUNTS_KEY  = 'auth.accounts'
const ACTIVE_ID_KEY = 'auth.activeId'
const LEGACY_TOKEN  = 'auth.token' // migrated to multi-account on first bootstrap

export const MAX_ACCOUNTS = 3

export interface AppUser {
  id: string
  email: string
  username: string | null
  display_name: string
  avatar_url: string | null
  bio: string | null
  onboarded: boolean
  [key: string]: unknown
}

export interface AccountRecord {
  id: string
  token: string
  user: AppUser
}

interface SendOtpResponse {
  sent: boolean
  dev_code?: string
}

interface VerifyOtpResponse {
  token: string
  user: AppUser
  is_new: boolean
}

interface AuthState {
  accounts:        AccountRecord[]
  activeId:        string | null
  user:            AppUser | null
  token:           string | null
  email:           string | null
  devCode:         string | null
  isNew:           boolean
  isLoading:       boolean
  isBootstrapping: boolean
  isAddingAccount: boolean
  error:           string | null

  bootstrap:        () => Promise<void>
  sendOtp:          (email: string) => Promise<void>
  verifyOtp:        (email: string, code: string) => Promise<void>
  switchAccount:    (id: string) => Promise<void>
  removeAccount:    (id: string) => Promise<void>
  signOut:          () => Promise<void>
  beginAddAccount:  () => void
  cancelAddAccount: () => void
  clearError:       () => void
}

async function persistAccounts(accounts: AccountRecord[]) {
  await AsyncStorage.setItem(ACCOUNTS_KEY, JSON.stringify(accounts))
}

export const useAuthStore = create<AuthState>((set, get) => ({
  accounts:        [],
  activeId:        null,
  user:            null,
  token:           null,
  email:           null,
  devCode:         null,
  isNew:           false,
  isLoading:       false,
  isBootstrapping: true,
  isAddingAccount: false,
  error:           null,

  bootstrap: async () => {
    try {
      let accounts: AccountRecord[] = []
      const stored = await AsyncStorage.getItem(ACCOUNTS_KEY)

      if (stored) {
        accounts = JSON.parse(stored) as AccountRecord[]
      } else {
        // Migrate from legacy single-token storage
        const oldToken = await AsyncStorage.getItem(LEGACY_TOKEN)
        if (oldToken) {
          api.setToken(oldToken)
          try {
            const res = await api.get<{ data: AppUser }>('/api/v1/users/me')
            accounts = [{ id: res.data.id, token: oldToken, user: res.data }]
            await persistAccounts(accounts)
            await AsyncStorage.setItem(ACTIVE_ID_KEY, accounts[0].id)
          } catch { /* expired — discard */ }
          await AsyncStorage.removeItem(LEGACY_TOKEN)
        }
      }

      if (!accounts.length) {
        set({ isBootstrapping: false })
        return
      }

      const savedId = await AsyncStorage.getItem(ACTIVE_ID_KEY)
      const active  = accounts.find(a => a.id === savedId) ?? accounts[0]

      api.setToken(active.token)
      try {
        const res     = await api.get<{ data: AppUser }>('/api/v1/users/me')
        const updated = accounts.map(a => a.id === active.id ? { ...a, user: res.data } : a)
        await persistAccounts(updated)
        set({ accounts: updated, activeId: active.id, token: active.token, user: res.data, isBootstrapping: false })
      } catch {
        // Active token expired — drop it, use next
        const rest = accounts.filter(a => a.id !== active.id)
        await persistAccounts(rest)
        if (!rest.length) {
          await AsyncStorage.removeItem(ACTIVE_ID_KEY)
          api.setToken(null)
          set({ accounts: [], activeId: null, token: null, user: null, isBootstrapping: false })
        } else {
          const next = rest[0]
          await AsyncStorage.setItem(ACTIVE_ID_KEY, next.id)
          api.setToken(next.token)
          set({ accounts: rest, activeId: next.id, token: next.token, user: next.user, isBootstrapping: false })
        }
      }
    } catch {
      set({ isBootstrapping: false })
    }
  },

  sendOtp: async (email) => {
    set({ isLoading: true, error: null, devCode: null })
    try {
      const res = await api.post<SendOtpResponse>('/api/v1/auth/send-otp', { email })
      set({ email, devCode: res.dev_code ?? null, isLoading: false })
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Failed to send code'
      set({ error: message, isLoading: false })
      throw err
    }
  },

  verifyOtp: async (email, code) => {
    set({ isLoading: true, error: null })
    try {
      const res = await api.post<VerifyOtpResponse>('/api/v1/auth/verify-otp', { email, code })

      const record: AccountRecord = { id: res.user.id, token: res.token, user: res.user }
      const prev    = get().accounts
      const updated = prev.some(a => a.id === record.id)
        ? prev.map(a => a.id === record.id ? record : a)
        : [...prev, record]

      await persistAccounts(updated)
      await AsyncStorage.setItem(ACTIVE_ID_KEY, record.id)
      api.setToken(res.token)

      set({
        accounts:        updated,
        activeId:        record.id,
        token:           res.token,
        user:            res.user,
        isNew:           res.is_new,
        devCode:         null,
        isLoading:       false,
        isAddingAccount: false,
      })
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Invalid code'
      set({ error: message, isLoading: false })
      throw err
    }
  },

  switchAccount: async (id) => {
    const { accounts } = get()
    const account = accounts.find(a => a.id === id)
    if (!account) return

    await AsyncStorage.setItem(ACTIVE_ID_KEY, id)
    api.setToken(account.token)
    set({ activeId: id, token: account.token, user: account.user })

    // Refresh in background
    try {
      const res     = await api.get<{ data: AppUser }>('/api/v1/users/me')
      const updated = get().accounts.map(a => a.id === id ? { ...a, user: res.data } : a)
      await persistAccounts(updated)
      set({ accounts: updated, user: res.data })
    } catch {
      await get().removeAccount(id)
    }
  },

  removeAccount: async (id) => {
    const { accounts, activeId } = get()
    const rest = accounts.filter(a => a.id !== id)
    await persistAccounts(rest)

    if (!rest.length) {
      await AsyncStorage.removeItem(ACTIVE_ID_KEY)
      api.setToken(null)
      set({ accounts: [], activeId: null, token: null, user: null })
    } else if (id === activeId) {
      const next = rest[0]
      await AsyncStorage.setItem(ACTIVE_ID_KEY, next.id)
      api.setToken(next.token)
      set({ accounts: rest, activeId: next.id, token: next.token, user: next.user })
    } else {
      set({ accounts: rest })
    }
  },

  signOut: async () => {
    const { activeId } = get()
    if (activeId) {
      await get().removeAccount(activeId)
    } else {
      await AsyncStorage.multiRemove([ACCOUNTS_KEY, ACTIVE_ID_KEY, LEGACY_TOKEN])
      api.setToken(null)
      set({ accounts: [], activeId: null, token: null, user: null })
    }
  },

  beginAddAccount: () => {
    if (get().accounts.length >= MAX_ACCOUNTS) return
    set({ isAddingAccount: true, email: null, devCode: null, error: null })
  },

  cancelAddAccount: () => {
    set({ isAddingAccount: false, email: null, devCode: null, error: null })
  },

  clearError: () => set({ error: null }),
}))
