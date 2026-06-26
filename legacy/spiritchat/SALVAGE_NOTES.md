# SpiritChat (legacy) — salvage notes

This is the original **SpiritChat / MySocialApp** project, imported as a **reference only**.
It is a cloud-first Turborepo (Threads + Telegram + TikTok style social app). We are
building a **decentralized P2P, end-to-end encrypted messenger** instead, so the cloud
"brain" gets removed and the UI gets reused.

> ⚠️ Nothing in this folder is the new app. It is a source we cherry-pick from.

## Original stack

- `apps/mobile` — Expo / React Native (Expo SDK 55, RN 0.83, React 19), Expo Router.
- `apps/api` — Go / Fiber backend (feed, posts, follows, WebSocket).
- `apps/worker` — Cloudflare Worker (file upload).
- `supabase/` — Supabase (Postgres + Auth + edge functions + migrations).
- `packages/types` — shared TypeScript types.

## ✅ Keep / reuse (UI & client work)

- **Liquid Glass design** — built with `expo-glass-effect` + `expo-blur`.
- **Screens** — chat, conversations (messages), contacts, profile, settings.
- **Components** — `MessageBubble`, `ChatInputBar`, `ConversationRow`, `Avatar`,
  `QrCodeModal`, `SearchBar`, `UserListItem`, `SettingsRow`.
- **QR scaffolding** — `app/settings/qr.tsx` + `QrCodeModal.tsx` + `expo-camera`
  (already wired for scanning QR codes) → reuse for contact exchange.
- Navigation (expo-router), animations (Reanimated, Skia), Zustand store.

## ❌ Remove / replace (the cloud "brain")

- `apps/api` (Go), `apps/worker` (Cloudflare), all of `supabase/`.
- `apps/mobile/lib/supabase.ts`, `apps/mobile/lib/api.ts` — cloud clients.
- `apps/mobile/app/(auth)/*` — phone/email OTP via Supabase Auth. Our identity model
  is a **local cryptographic keypair + QR**, not server accounts.
- `apps/mobile/app/settings/cloud-password.tsx` — cloud backup, against our model.
- Reconsider `expo-updates` (OTA via Expo cloud) and `expo-notifications` (cloud push).

## 🔄 Architectural decision

Keep the JS UI, but move all security-critical logic — identity, keys, encryption,
P2P (libp2p) — into a **native module (Rust)** so sensitive code lives in the signed
native binary, not in hot-updatable JS. This requires moving from **Expo Go** to an
**Expo development build (dev client)**, because Expo Go cannot load custom native code.

## 🔐 Cleanup TODO (old cloud resources)

The committed code contains no high-risk secrets (no `service_role` key; only a
*publishable* Supabase anon key + project URL, which are designed to be public). Since
we are dropping the cloud, pause/delete when convenient:

- Supabase project `vscasdbufemkzrjyqddn`
- Cloudflare Worker `mysocialapp-upload.*.workers.dev`
