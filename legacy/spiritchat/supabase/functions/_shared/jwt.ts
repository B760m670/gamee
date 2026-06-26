// HS256 JWT signing / verification, matching the Go backend's TokenIssuer:
// the "sub" claim holds the user ID. Using the same shared JWT_SECRET means
// tokens issued here are also accepted by the Go API if it is later deployed.
import {
  create,
  verify,
  getNumericDate,
} from "https://deno.land/x/djwt@v3.0.2/mod.ts"

async function hmacKey(secret: string): Promise<CryptoKey> {
  return await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  )
}

export async function signToken(
  secret: string,
  userId: string,
  ttlSeconds: number,
): Promise<string> {
  const key = await hmacKey(secret)
  return await create(
    { alg: "HS256", typ: "JWT" },
    { sub: userId, iat: getNumericDate(0), exp: getNumericDate(ttlSeconds) },
    key,
  )
}

// verifyToken returns the "sub" (user ID) on success, or null if the token is
// missing, malformed, expired, or signed with the wrong key.
export async function verifyToken(
  secret: string,
  token: string,
): Promise<string | null> {
  try {
    const key = await hmacKey(secret)
    const payload = await verify(token, key)
    const sub = payload.sub
    return typeof sub === "string" && sub.length > 0 ? sub : null
  } catch {
    return null
  }
}
