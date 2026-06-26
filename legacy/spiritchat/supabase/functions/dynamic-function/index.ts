// Single-file Edge Function "api" — auth + users/me.
// Self-contained (no _shared imports) so it can be pasted directly into the
// Supabase Dashboard edge-function editor.
import { createClient } from "https://esm.sh/@supabase/supabase-js@2"
import {
  create,
  verify,
  getNumericDate,
} from "https://deno.land/x/djwt@v3.0.2/mod.ts"

// ── Config ────────────────────────────────────────────────────────────────────

const SUPABASE_URL        = Deno.env.get("SUPABASE_URL")!
const SERVICE_KEY         = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!
const JWT_SECRET          = Deno.env.get("JWT_SECRET")!
const EMAIL_PROVIDER      = Deno.env.get("EMAIL_PROVIDER") ?? "console"
const RESEND_API_KEY      = Deno.env.get("RESEND_API_KEY") ?? ""
const RESEND_FROM         = Deno.env.get("RESEND_FROM") ?? "MySocialApp <onboarding@resend.dev>"
const OTP_DEV_EXPOSE_CODE = Deno.env.get("OTP_DEV_EXPOSE_CODE") === "true"

const OTP_TTL_SECONDS             = 300
const OTP_RESEND_COOLDOWN_SECONDS = 60
const OTP_MAX_ATTEMPTS            = 5
const OTP_CODE_LENGTH             = 6
const TOKEN_TTL_SECONDS           = 60 * 60 * 24 * 30 // 30 days

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const CORS = {
  "Access-Control-Allow-Origin":  "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
}

const db = createClient(SUPABASE_URL, SERVICE_KEY)

// ── Helpers ───────────────────────────────────────────────────────────────────

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...CORS, "Content-Type": "application/json" },
  })
}

function randomCode(length: number): string {
  const digits = "0123456789"
  let out = ""
  while (out.length < length) {
    const buf = new Uint8Array(length * 2)
    crypto.getRandomValues(buf)
    for (const b of buf) {
      if (b < 250 && out.length < length) out += digits[b % 10]
    }
  }
  return out
}

async function hmacKey(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  )
}

async function signToken(userId: string): Promise<string> {
  const key = await hmacKey(JWT_SECRET)
  // `role`/`aud` make the token a valid Supabase access token, so the mobile
  // client can authorise Realtime subscriptions and RLS resolves auth.uid().
  return create(
    { alg: "HS256", typ: "JWT" },
    {
      sub:  userId,
      role: "authenticated",
      aud:  "authenticated",
      iat:  getNumericDate(0),
      exp:  getNumericDate(TOKEN_TTL_SECONDS),
    },
    key,
  )
}

async function authUserId(req: Request): Promise<string | null> {
  const header = req.headers.get("Authorization") ?? ""
  const match  = header.match(/^Bearer\s+(.+)$/i)
  if (!match) return null
  try {
    const key     = await hmacKey(JWT_SECRET)
    const payload = await verify(match[1], key)
    const sub     = payload.sub
    return typeof sub === "string" && sub.length > 0 ? sub : null
  } catch {
    return null
  }
}

async function sendEmail(to: string, code: string): Promise<void> {
  const subject = "Your verification code"
  const text    = `Your MySocialApp verification code is: ${code}\n\nExpires in 5 minutes. Do not share it with anyone.`

  if (EMAIL_PROVIDER !== "resend") {
    console.log(`[EMAIL:console] to=${to} code=${code}`)
    return
  }
  const res = await fetch("https://api.resend.com/emails", {
    method:  "POST",
    headers: { Authorization: `Bearer ${RESEND_API_KEY}`, "Content-Type": "application/json" },
    body:    JSON.stringify({ from: RESEND_FROM, to: [to], subject, text }),
  })
  if (!res.ok) throw new Error(`resend ${res.status}: ${await res.text()}`)
}

// ── Route handlers ────────────────────────────────────────────────────────────

async function sendOtp(email: string): Promise<Response> {
  const now = Date.now()
  const { data: existing } = await db
    .from("otp_codes").select("resend_after").eq("email", email).maybeSingle()
  if (existing && new Date(existing.resend_after).getTime() > now) {
    return json({ message: "please wait before requesting a new code" }, 429)
  }

  const code = randomCode(OTP_CODE_LENGTH)
  const { error } = await db.from("otp_codes").upsert({
    email,
    code,
    expires_at:   new Date(now + OTP_TTL_SECONDS * 1000).toISOString(),
    resend_after: new Date(now + OTP_RESEND_COOLDOWN_SECONDS * 1000).toISOString(),
    attempts:     0,
  })
  if (error) { console.error("store code:", error); return json({ message: "failed to generate code" }, 500) }

  try {
    await sendEmail(email, code)
  } catch (e) {
    console.error("send email:", e)
    if (!OTP_DEV_EXPOSE_CODE) {
      return json({ message: "failed to send email" }, 502)
    }
    // dev mode: email error is non-fatal, code is returned in response
  }

  const resp: Record<string, unknown> = { sent: true }
  if (OTP_DEV_EXPOSE_CODE) resp.dev_code = code
  return json(resp)
}

async function verifyOtp(email: string, code: string): Promise<Response> {
  const { data: row } = await db
    .from("otp_codes").select("*").eq("email", email).maybeSingle()

  if (!row || new Date(row.expires_at).getTime() < Date.now()) {
    return json({ message: "code expired or not found" }, 401)
  }
  if (row.code !== code) {
    const attempts = (row.attempts ?? 0) + 1
    if (attempts >= OTP_MAX_ATTEMPTS) {
      await db.from("otp_codes").delete().eq("email", email)
      return json({ message: "too many attempts, request a new code" }, 429)
    }
    await db.from("otp_codes").update({ attempts }).eq("email", email)
    return json({ message: "incorrect code" }, 401)
  }

  await db.from("otp_codes").delete().eq("email", email)

  let { data: user } = await db.from("users").select("*").eq("email", email).maybeSingle()
  let isNew = false
  if (!user) {
    const { data: created, error } = await db
      .from("users")
      .insert({ id: crypto.randomUUID(), email, onboarded: false })
      .select().single()
    if (error) { console.error("create user:", error); return json({ message: "failed to create user" }, 500) }
    user  = created
    isNew = true
  }

  const token = await signToken(user.id as string)
  return json({ token, user, is_new: isNew })
}

async function getMe(req: Request): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  const { data: user } = await db.from("users").select("*").eq("id", uid).maybeSingle()
  if (!user) return json({ message: "user not found" }, 404)
  return json({ data: user })
}

// Public profile fields exposed to other authenticated users (never email).
const PUBLIC_USER_FIELDS = "id, username, display_name, avatar_url, bio"

async function searchUsers(req: Request, rawQuery: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)

  // Usernames are [a-z0-9_]; strip a leading @ and anything else so the
  // ilike pattern can't be abused and matches the username namespace.
  const term = rawQuery.trim().toLowerCase().replace(/^@/, "").replace(/[^a-z0-9_]/g, "")
  if (term.length < 2) return json({ data: [] })

  const { data, error } = await db
    .from("users")
    .select(PUBLIC_USER_FIELDS)
    .ilike("username", `${term}%`)
    .not("username", "is", null)
    .neq("id", uid)
    .order("username", { ascending: true })
    .limit(20)
  if (error) { console.error("search users:", error); return json({ message: "search failed" }, 500) }
  return json({ data: data ?? [] })
}

async function getUserById(req: Request, id: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  const { data, error } = await db
    .from("users").select(PUBLIC_USER_FIELDS).eq("id", id).maybeSingle()
  if (error) { console.error("get user:", error); return json({ message: "lookup failed" }, 500) }
  if (!data) return json({ message: "user not found" }, 404)
  return json({ data })
}

async function updateMe(req: Request): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)

  const body    = await req.json().catch(() => ({})) as Record<string, unknown>
  const allowed = ["display_name", "bio", "avatar_url", "website", "username", "onboarded"]
  const update: Record<string, unknown> = {}
  for (const k of allowed) if (k in body) update[k] = body[k]
  if (Object.keys(update).length === 0) return json({ message: "no updatable fields provided" }, 400)

  const { data: updated, error } = await db
    .from("users").update(update).eq("id", uid).select().single()
  if (error) return json({ message: error.message }, 500)
  return json({ data: updated, message: "profile updated" })
}

// ── Messaging ─────────────────────────────────────────────────────────────────

const MESSAGE_PAGE_SIZE = 30

// Resolve (or lazily create) the direct conversation between the caller and
// another user. Returns the conversation id + the other user's public profile.
async function openConversation(req: Request, otherId: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  if (otherId === uid) return json({ message: "cannot message yourself" }, 400)

  const { data: other } = await db
    .from("users").select(PUBLIC_USER_FIELDS).eq("id", otherId).maybeSingle()
  if (!other) return json({ message: "user not found" }, 404)

  const { data: cid, error } = await db
    .rpc("get_or_create_direct_conversation", { p_me: uid, p_other: otherId })
  if (error) { console.error("open conversation:", error); return json({ message: "failed to open chat" }, 500) }

  return json({ data: { conversation_id: cid, other } })
}

// List the caller's conversations that have at least one message, newest first,
// with the other participant, last message preview and unread count.
async function listConversations(req: Request): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)

  const { data: parts } = await db
    .from("conversation_participants")
    .select("conversation_id, last_read_at")
    .eq("user_id", uid)
  if (!parts || parts.length === 0) return json({ data: [] })

  const ids      = parts.map((p) => p.conversation_id)
  const readMap  = new Map(parts.map((p) => [p.conversation_id, p.last_read_at]))

  const { data: convs } = await db
    .from("conversations")
    .select("id, last_message_at")
    .in("id", ids)
    .not("last_message_at", "is", null)
    .order("last_message_at", { ascending: false })
  if (!convs || convs.length === 0) return json({ data: [] })

  const result = []
  for (const c of convs) {
    const { data: others } = await db
      .from("conversation_participants")
      .select("user_id")
      .eq("conversation_id", c.id)
      .neq("user_id", uid)
      .limit(1)
    const otherId = others?.[0]?.user_id
    let other = null
    if (otherId) {
      const { data: ou } = await db.from("users").select(PUBLIC_USER_FIELDS).eq("id", otherId).maybeSingle()
      other = ou ?? null
    }

    const { data: last } = await db
      .from("messages")
      .select("id, sender_id, content, media_url, created_at")
      .eq("conversation_id", c.id)
      .eq("is_deleted", false)
      .order("created_at", { ascending: false })
      .limit(1)
      .maybeSingle()

    const lastRead = readMap.get(c.id)
    let unread = 0
    if (last) {
      const { count } = await db
        .from("messages")
        .select("id", { count: "exact", head: true })
        .eq("conversation_id", c.id)
        .neq("sender_id", uid)
        .gt("created_at", lastRead ?? "1970-01-01")
      unread = count ?? 0
    }

    result.push({ conversation_id: c.id, other, last_message: last, unread })
  }

  return json({ data: result })
}

async function listMessages(req: Request, conversationId: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  if (!(await isParticipant(conversationId, uid))) return json({ message: "forbidden" }, 403)

  const params = new URL(req.url).searchParams
  const before = params.get("before")
  const after  = params.get("after")

  let query = db
    .from("messages")
    .select("id, conversation_id, sender_id, content, media_url, client_id, is_deleted, created_at")
    .eq("conversation_id", conversationId)
    .order("created_at", { ascending: false })

  if (after) {
    // Polling: return messages newer than the cursor, up to 100 at a time.
    query = query.gt("created_at", after).limit(100)
  } else {
    query = query.limit(MESSAGE_PAGE_SIZE)
    if (before) query = query.lt("created_at", before)
  }

  const { data, error } = await query
  if (error) { console.error("list messages:", error); return json({ message: "failed to load" }, 500) }

  // Read cursor of the *other* participant — lets the client show read ticks.
  const { data: otherPart } = await db
    .from("conversation_participants")
    .select("last_read_at")
    .eq("conversation_id", conversationId)
    .neq("user_id", uid)
    .limit(1)
    .maybeSingle()

  return json({ data: data ?? [], other_last_read_at: otherPart?.last_read_at ?? null })
}

async function sendMessage(req: Request, conversationId: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  if (!(await isParticipant(conversationId, uid))) return json({ message: "forbidden" }, 403)

  const body     = await req.json().catch(() => ({})) as Record<string, unknown>
  const content  = typeof body.content === "string" ? body.content.trim() : ""
  const mediaUrl = typeof body.media_url === "string" ? body.media_url : null
  const clientId = typeof body.client_id === "string" ? body.client_id : null
  if (!content && !mediaUrl) return json({ message: "message is empty" }, 400)

  // Idempotent: if this client_id was already stored, return the existing row.
  if (clientId) {
    const { data: existing } = await db
      .from("messages").select("*").eq("conversation_id", conversationId).eq("client_id", clientId).maybeSingle()
    if (existing) return json({ data: existing })
  }

  const { data, error } = await db
    .from("messages")
    .insert({ conversation_id: conversationId, sender_id: uid, content: content || null, media_url: mediaUrl, client_id: clientId })
    .select().single()
  if (error) { console.error("send message:", error); return json({ message: "failed to send" }, 500) }

  return json({ data })
}

async function markRead(req: Request, conversationId: string): Promise<Response> {
  const uid = await authUserId(req)
  if (!uid) return json({ message: "unauthorized" }, 401)
  if (!(await isParticipant(conversationId, uid))) return json({ message: "forbidden" }, 403)

  const { error } = await db
    .from("conversation_participants")
    .update({ last_read_at: new Date().toISOString() })
    .eq("conversation_id", conversationId)
    .eq("user_id", uid)
  if (error) { console.error("mark read:", error); return json({ message: "failed" }, 500) }
  return json({ ok: true })
}

async function isParticipant(conversationId: string, uid: string): Promise<boolean> {
  const { data } = await db
    .from("conversation_participants")
    .select("user_id")
    .eq("conversation_id", conversationId)
    .eq("user_id", uid)
    .maybeSingle()
  return !!data
}

// ── Router ────────────────────────────────────────────────────────────────────

Deno.serve(async (req: Request) => {
  if (req.method === "OPTIONS") return new Response("ok", { headers: CORS })

  const path = new URL(req.url).pathname
  try {
    if (req.method === "POST" && path.endsWith("/auth/send-otp")) {
      const { email } = await req.json()
      if (typeof email !== "string" || !EMAIL_RE.test(email))
        return json({ message: "invalid email address" }, 400)
      return sendOtp(email.trim().toLowerCase())
    }
    if (req.method === "POST" && path.endsWith("/auth/verify-otp")) {
      const { email, code } = await req.json()
      if (typeof email !== "string" || !EMAIL_RE.test(email))
        return json({ message: "invalid email address" }, 400)
      if (!code) return json({ message: "code is required" }, 400)
      return verifyOtp(email.trim().toLowerCase(), String(code))
    }
    if (path.endsWith("/users/me")) {
      if (req.method === "GET") return getMe(req)
      if (req.method === "PUT")  return updateMe(req)
    }
    if (req.method === "GET" && path.endsWith("/users/search")) {
      const q = new URL(req.url).searchParams.get("q") ?? ""
      return searchUsers(req, q)
    }

    // ── Messaging ──
    if (req.method === "GET" && path.endsWith("/conversations")) {
      return listConversations(req)
    }
    if (req.method === "POST" && path.endsWith("/conversations/with")) {
      const { user_id } = await req.json().catch(() => ({}))
      if (typeof user_id !== "string") return json({ message: "user_id required" }, 400)
      return openConversation(req, user_id)
    }
    const convMsgsMatch = path.match(/\/conversations\/([^/]+)\/messages$/)
    if (convMsgsMatch) {
      const cid = decodeURIComponent(convMsgsMatch[1])
      if (req.method === "GET")  return listMessages(req, cid)
      if (req.method === "POST") return sendMessage(req, cid)
    }
    const convReadMatch = path.match(/\/conversations\/([^/]+)\/read$/)
    if (req.method === "POST" && convReadMatch) {
      return markRead(req, decodeURIComponent(convReadMatch[1]))
    }

    // GET /users/:id — public profile lookup (after the more specific routes above)
    const userIdMatch = path.match(/\/users\/([^/]+)$/)
    if (req.method === "GET" && userIdMatch) {
      return getUserById(req, decodeURIComponent(userIdMatch[1]))
    }
    return json({ message: "not found" }, 404)
  } catch (e) {
    console.error("unhandled:", e)
    return json({ message: "internal error" }, 500)
  }
})
