-- Migration 015: Direct-message foundation (1-on-1 chats + Realtime).
--
-- Reuses the existing conversations / conversation_participants / messages
-- tables. Adds: idempotent send support (client_id), a get-or-create RPC for
-- direct chats, a trigger that keeps conversations.last_message_at fresh, and
-- Realtime publication so the mobile client receives live inserts/updates.

-- ── 1. Idempotent / optimistic send: client-generated id ──────────────────────
ALTER TABLE public.messages ADD COLUMN IF NOT EXISTS client_id UUID;

-- One row per (conversation, client_id): a retried send can't duplicate.
CREATE UNIQUE INDEX IF NOT EXISTS messages_conv_client_uniq
  ON public.messages (conversation_id, client_id)
  WHERE client_id IS NOT NULL;

-- ── 2. Get-or-create a direct conversation between two users ──────────────────
-- Called from the Edge Function (service role) with both ids explicit.
CREATE OR REPLACE FUNCTION public.get_or_create_direct_conversation(p_me UUID, p_other UUID)
RETURNS UUID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  cid UUID;
BEGIN
  SELECT c.id INTO cid
  FROM public.conversations c
  JOIN public.conversation_participants p1 ON p1.conversation_id = c.id AND p1.user_id = p_me
  JOIN public.conversation_participants p2 ON p2.conversation_id = c.id AND p2.user_id = p_other
  WHERE c.type = 'direct'
  LIMIT 1;

  IF cid IS NOT NULL THEN
    RETURN cid;
  END IF;

  INSERT INTO public.conversations (type) VALUES ('direct') RETURNING id INTO cid;
  INSERT INTO public.conversation_participants (conversation_id, user_id)
    VALUES (cid, p_me), (cid, p_other);
  RETURN cid;
END;
$$;

-- ── 3. Keep conversations.last_message_at in sync with new messages ───────────
CREATE OR REPLACE FUNCTION public.bump_conversation_last_message()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
BEGIN
  UPDATE public.conversations
     SET last_message_at = NEW.created_at
   WHERE id = NEW.conversation_id;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_bump_conv_last_message ON public.messages;
CREATE TRIGGER trg_bump_conv_last_message
  AFTER INSERT ON public.messages
  FOR EACH ROW EXECUTE FUNCTION public.bump_conversation_last_message();

-- ── 4. Realtime: publish the tables the client subscribes to ──────────────────
-- Guarded so re-running the migration is safe.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_publication_tables
    WHERE pubname = 'supabase_realtime' AND schemaname = 'public' AND tablename = 'messages'
  ) THEN
    ALTER PUBLICATION supabase_realtime ADD TABLE public.messages;
  END IF;

  IF NOT EXISTS (
    SELECT 1 FROM pg_publication_tables
    WHERE pubname = 'supabase_realtime' AND schemaname = 'public' AND tablename = 'conversations'
  ) THEN
    ALTER PUBLICATION supabase_realtime ADD TABLE public.conversations;
  END IF;

  IF NOT EXISTS (
    SELECT 1 FROM pg_publication_tables
    WHERE pubname = 'supabase_realtime' AND schemaname = 'public' AND tablename = 'conversation_participants'
  ) THEN
    ALTER PUBLICATION supabase_realtime ADD TABLE public.conversation_participants;
  END IF;
END;
$$;

-- Full row image so RLS can be evaluated on UPDATE/DELETE for Realtime.
ALTER TABLE public.messages                  REPLICA IDENTITY FULL;
ALTER TABLE public.conversations             REPLICA IDENTITY FULL;
ALTER TABLE public.conversation_participants REPLICA IDENTITY FULL;
