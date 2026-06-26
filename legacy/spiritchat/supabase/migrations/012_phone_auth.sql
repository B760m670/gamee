-- =============================================================================
-- 012_phone_auth.sql
-- Switch to self-hosted phone OTP authentication (Telegram-style).
-- The Go backend now owns auth: it generates user IDs, signs its own JWTs,
-- and accesses the DB via the service key (bypassing RLS). We therefore
-- decouple public.users from Supabase's auth.users.
-- =============================================================================

-- 1. Drop the Supabase Auth trigger that mirrored auth.users → public.users.
DROP TRIGGER IF EXISTS on_auth_user_created ON auth.users;
DROP FUNCTION IF EXISTS public.handle_new_user();

-- 2. Decouple public.users.id from auth.users(id).
--    The backend will generate UUIDs itself on first verification.
ALTER TABLE public.users DROP CONSTRAINT IF EXISTS users_id_fkey;
ALTER TABLE public.users ALTER COLUMN id SET DEFAULT uuid_generate_v4();

-- 3. Phone number is now the primary identity (E.164 format, e.g. +14155550123).
ALTER TABLE public.users ADD COLUMN IF NOT EXISTS phone TEXT;
ALTER TABLE public.users ADD CONSTRAINT users_phone_key UNIQUE (phone);
CREATE INDEX IF NOT EXISTS idx_users_phone ON public.users (phone);

-- 4. Username is optional until the user picks one during onboarding.
ALTER TABLE public.users ALTER COLUMN username DROP NOT NULL;

-- 5. Track onboarding completion so the client knows whether to show setup.
ALTER TABLE public.users ADD COLUMN IF NOT EXISTS onboarded BOOLEAN NOT NULL DEFAULT FALSE;
