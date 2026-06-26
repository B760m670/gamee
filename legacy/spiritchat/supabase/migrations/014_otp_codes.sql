-- Migration 014: OTP code storage for email auth (replaces Redis).
-- The Edge Function "api" reads/writes this table with the service role key,
-- which bypasses RLS. RLS is enabled with no policies so the anon/public API
-- cannot touch codes.

CREATE TABLE IF NOT EXISTS public.otp_codes (
  email        TEXT PRIMARY KEY,
  code         TEXT NOT NULL,
  expires_at   TIMESTAMPTZ NOT NULL,
  resend_after TIMESTAMPTZ NOT NULL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE public.otp_codes ENABLE ROW LEVEL SECURITY;
-- No policies on purpose: only the service role (Edge Functions) may access it.
