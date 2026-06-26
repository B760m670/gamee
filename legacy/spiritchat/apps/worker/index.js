const CORS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Authorization, Content-Type',
}

const ALLOWED_TYPES = [
  'image/jpeg', 'image/png', 'image/webp', 'image/gif',
  'video/mp4', 'video/quicktime', 'video/mov',
]

const WORKER_URL = 'https://mysocialapp-upload.metrvsapke.workers.dev'

function b64url(str) {
  return str.replace(/-/g, '+').replace(/_/g, '/')
}

async function verifyJWT(token, secret) {
  const parts = token.split('.')
  if (parts.length !== 3) throw new Error('malformed token')

  const [headerB64, payloadB64, sigB64] = parts
  const encoder = new TextEncoder()

  const key = await crypto.subtle.importKey(
    'raw',
    encoder.encode(secret),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['verify'],
  )

  const signingInput = `${headerB64}.${payloadB64}`
  const sigBytes = Uint8Array.from(atob(b64url(sigB64)), c => c.charCodeAt(0))
  const valid = await crypto.subtle.verify('HMAC', key, sigBytes, encoder.encode(signingInput))
  if (!valid) throw new Error('invalid signature')

  const payload = JSON.parse(atob(b64url(payloadB64)))
  if (payload.exp && payload.exp < Date.now() / 1000) throw new Error('token expired')

  return payload
}

export default {
  async fetch(request, env) {
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: CORS })
    }

    // GET /<userId>/<filename> — serve file from R2
    if (request.method === 'GET') {
      const url = new URL(request.url)
      const key = url.pathname.slice(1)
      if (!key) return new Response('Not found', { status: 404, headers: CORS })

      const object = await env.MEDIA_BUCKET.get(key)
      if (!object) return new Response('Not found', { status: 404, headers: CORS })

      return new Response(object.body, {
        headers: {
          ...CORS,
          'Content-Type': object.httpMetadata?.contentType ?? 'application/octet-stream',
          'Cache-Control': 'public, max-age=31536000',
        },
      })
    }

    if (request.method !== 'POST') {
      return new Response('Method Not Allowed', { status: 405, headers: CORS })
    }

    // Verify JWT (same HS256 secret as the Go API)
    const auth = request.headers.get('Authorization')
    if (!auth?.startsWith('Bearer ')) {
      return new Response('Unauthorized', { status: 401, headers: CORS })
    }

    const jwtSecret = env.JWT_SECRET
    if (!jwtSecret) {
      return new Response('Worker misconfigured: JWT_SECRET missing', { status: 500, headers: CORS })
    }

    let userId
    try {
      const payload = await verifyJWT(auth.slice(7), jwtSecret)
      userId = payload.sub
      if (!userId) throw new Error('missing sub claim')
    } catch (err) {
      return new Response('Unauthorized: ' + err.message, { status: 401, headers: CORS })
    }

    // Parse multipart form
    let formData
    try {
      formData = await request.formData()
    } catch {
      return new Response('Invalid form data', { status: 400, headers: CORS })
    }

    const file = formData.get('file')
    if (!file) return new Response('No file provided', { status: 400, headers: CORS })

    if (!ALLOWED_TYPES.includes(file.type)) {
      return new Response(`File type not allowed: ${file.type}`, { status: 400, headers: CORS })
    }

    if (file.size > 100 * 1024 * 1024) {
      return new Response('File too large (max 100MB)', { status: 400, headers: CORS })
    }

    const ext  = file.name.split('.').pop()?.toLowerCase() || 'jpg'
    const path = `${userId}/avatar.${ext}`
    const ver  = Date.now()

    try {
      await env.MEDIA_BUCKET.put(path, file.stream(), {
        httpMetadata: { contentType: file.type },
      })
    } catch (err) {
      return new Response(`R2 upload failed: ${err.message}`, { status: 500, headers: CORS })
    }

    const workerUrl = env.WORKER_URL ?? WORKER_URL
    return Response.json({ url: `${workerUrl}/${path}?v=${ver}` }, { headers: CORS })
  },
}
