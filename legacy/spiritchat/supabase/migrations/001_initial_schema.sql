-- =============================================================================
-- Initial Schema for MySocialApp
-- =============================================================================

-- Extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

-- =============================================================================
-- TABLES
-- =============================================================================

-- users (mirrors auth.users with additional profile fields)
CREATE TABLE IF NOT EXISTS public.users (
    id             UUID PRIMARY KEY REFERENCES auth.users(id) ON DELETE CASCADE,
    username       TEXT UNIQUE NOT NULL,
    display_name   TEXT NOT NULL DEFAULT '',
    bio            TEXT,
    avatar_url     TEXT,
    website        TEXT,
    follower_count  INTEGER NOT NULL DEFAULT 0,
    following_count INTEGER NOT NULL DEFAULT 0,
    post_count      INTEGER NOT NULL DEFAULT 0,
    is_verified     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_username ON public.users (username);

-- follows
CREATE TABLE IF NOT EXISTS public.follows (
    follower_id  UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    following_id UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (follower_id, following_id),
    CONSTRAINT no_self_follow CHECK (follower_id <> following_id)
);

CREATE INDEX IF NOT EXISTS idx_follows_following_id ON public.follows (following_id);

-- media
CREATE TABLE IF NOT EXISTS public.media (
    id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    uploader_id   UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    url           TEXT NOT NULL,
    thumbnail_url TEXT,
    type          TEXT NOT NULL CHECK (type IN ('image', 'video')),
    duration_sec  NUMERIC,
    width         INTEGER,
    height        INTEGER,
    size_bytes    BIGINT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_media_uploader ON public.media (uploader_id);

-- posts
CREATE TABLE IF NOT EXISTS public.posts (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    author_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    content      TEXT NOT NULL CHECK (char_length(content) <= 500),
    parent_id    UUID REFERENCES public.posts(id) ON DELETE SET NULL,
    root_id      UUID REFERENCES public.posts(id) ON DELETE SET NULL,
    media_ids    UUID[] NOT NULL DEFAULT '{}',
    like_count   INTEGER NOT NULL DEFAULT 0,
    reply_count  INTEGER NOT NULL DEFAULT 0,
    repost_count INTEGER NOT NULL DEFAULT 0,
    view_count   INTEGER NOT NULL DEFAULT 0,
    is_deleted   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_posts_author_id     ON public.posts (author_id);
CREATE INDEX IF NOT EXISTS idx_posts_parent_id     ON public.posts (parent_id);
CREATE INDEX IF NOT EXISTS idx_posts_root_id       ON public.posts (root_id);
CREATE INDEX IF NOT EXISTS idx_posts_created_at    ON public.posts (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_posts_content_gin   ON public.posts USING GIN (to_tsvector('english', content));
CREATE INDEX IF NOT EXISTS idx_posts_content_trgm  ON public.posts USING GIN (content gin_trgm_ops);

-- likes
CREATE TABLE IF NOT EXISTS public.likes (
    user_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    post_id    UUID NOT NULL REFERENCES public.posts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, post_id)
);

CREATE INDEX IF NOT EXISTS idx_likes_post_id ON public.likes (post_id);

-- reposts
CREATE TABLE IF NOT EXISTS public.reposts (
    user_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    post_id    UUID NOT NULL REFERENCES public.posts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, post_id)
);

CREATE INDEX IF NOT EXISTS idx_reposts_post_id ON public.reposts (post_id);

-- stories
CREATE TABLE IF NOT EXISTS public.stories (
    id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    media_id   UUID NOT NULL REFERENCES public.media(id) ON DELETE CASCADE,
    type       TEXT NOT NULL CHECK (type IN ('image', 'video')),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours'),
    view_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_stories_user_id    ON public.stories (user_id);
CREATE INDEX IF NOT EXISTS idx_stories_expires_at ON public.stories (expires_at);

-- story_views
CREATE TABLE IF NOT EXISTS public.story_views (
    story_id  UUID NOT NULL REFERENCES public.stories(id) ON DELETE CASCADE,
    viewer_id UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    viewed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (story_id, viewer_id)
);

-- conversations
CREATE TABLE IF NOT EXISTS public.conversations (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    type            TEXT NOT NULL CHECK (type IN ('direct', 'group')),
    name            TEXT,
    avatar_url      TEXT,
    last_message_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- conversation_participants
CREATE TABLE IF NOT EXISTS public.conversation_participants (
    conversation_id UUID NOT NULL REFERENCES public.conversations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    last_read_at    TIMESTAMPTZ,
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (conversation_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_conv_participants_user ON public.conversation_participants (user_id);

-- messages
CREATE TABLE IF NOT EXISTS public.messages (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id UUID NOT NULL REFERENCES public.conversations(id) ON DELETE CASCADE,
    sender_id       UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    content         TEXT,
    media_url       TEXT,
    reply_to_id     UUID REFERENCES public.messages(id) ON DELETE SET NULL,
    is_deleted      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT message_has_content CHECK (content IS NOT NULL OR media_url IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation ON public.messages (conversation_id, created_at DESC);

-- channels
CREATE TABLE IF NOT EXISTS public.channels (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name         TEXT NOT NULL,
    handle       TEXT UNIQUE NOT NULL,
    description  TEXT,
    avatar_url   TEXT,
    owner_id     UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    member_count INTEGER NOT NULL DEFAULT 1,
    is_public    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_channels_handle   ON public.channels (handle);
CREATE INDEX IF NOT EXISTS idx_channels_owner_id ON public.channels (owner_id);

-- channel_members
CREATE TABLE IF NOT EXISTS public.channel_members (
    channel_id UUID NOT NULL REFERENCES public.channels(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member')),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_channel_members_user ON public.channel_members (user_id);

-- channel_posts
CREATE TABLE IF NOT EXISTS public.channel_posts (
    id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    channel_id UUID NOT NULL REFERENCES public.channels(id) ON DELETE CASCADE,
    sender_id  UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    content    TEXT,
    media_url  TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT channel_post_has_content CHECK (content IS NOT NULL OR media_url IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS idx_channel_posts_channel ON public.channel_posts (channel_id, created_at DESC);

-- notifications
CREATE TABLE IF NOT EXISTS public.notifications (
    id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id    UUID NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    type       TEXT NOT NULL CHECK (type IN ('like', 'follow', 'reply', 'mention', 'dm', 'repost', 'channel_post')),
    actor_id   UUID REFERENCES public.users(id) ON DELETE SET NULL,
    post_id    UUID REFERENCES public.posts(id) ON DELETE SET NULL,
    message_id UUID REFERENCES public.messages(id) ON DELETE SET NULL,
    is_read    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_notifications_user         ON public.notifications (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread  ON public.notifications (user_id) WHERE is_read = FALSE;

-- =============================================================================
-- ROW LEVEL SECURITY
-- =============================================================================

ALTER TABLE public.users                   ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.follows                 ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.media                   ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.posts                   ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.likes                   ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.reposts                 ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.stories                 ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.story_views             ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.conversations           ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.conversation_participants ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.messages                ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.channels                ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.channel_members         ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.channel_posts           ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.notifications           ENABLE ROW LEVEL SECURITY;

-- ─── users policies ───────────────────────────────────────────────────────────
CREATE POLICY "users_select_all"
    ON public.users FOR SELECT USING (TRUE);

CREATE POLICY "users_insert_own"
    ON public.users FOR INSERT WITH CHECK (auth.uid() = id);

CREATE POLICY "users_update_own"
    ON public.users FOR UPDATE USING (auth.uid() = id);

-- ─── follows policies ─────────────────────────────────────────────────────────
CREATE POLICY "follows_select_all"
    ON public.follows FOR SELECT USING (TRUE);

CREATE POLICY "follows_insert_own"
    ON public.follows FOR INSERT WITH CHECK (auth.uid() = follower_id);

CREATE POLICY "follows_delete_own"
    ON public.follows FOR DELETE USING (auth.uid() = follower_id);

-- ─── media policies ───────────────────────────────────────────────────────────
CREATE POLICY "media_select_all"
    ON public.media FOR SELECT USING (TRUE);

CREATE POLICY "media_insert_own"
    ON public.media FOR INSERT WITH CHECK (auth.uid() = uploader_id);

CREATE POLICY "media_delete_own"
    ON public.media FOR DELETE USING (auth.uid() = uploader_id);

-- ─── posts policies ───────────────────────────────────────────────────────────
CREATE POLICY "posts_select_not_deleted"
    ON public.posts FOR SELECT USING (is_deleted = FALSE);

CREATE POLICY "posts_insert_own"
    ON public.posts FOR INSERT WITH CHECK (auth.uid() = author_id);

CREATE POLICY "posts_update_own"
    ON public.posts FOR UPDATE USING (auth.uid() = author_id);

-- ─── likes policies ───────────────────────────────────────────────────────────
CREATE POLICY "likes_select_all"
    ON public.likes FOR SELECT USING (TRUE);

CREATE POLICY "likes_insert_own"
    ON public.likes FOR INSERT WITH CHECK (auth.uid() = user_id);

CREATE POLICY "likes_delete_own"
    ON public.likes FOR DELETE USING (auth.uid() = user_id);

-- ─── reposts policies ─────────────────────────────────────────────────────────
CREATE POLICY "reposts_select_all"
    ON public.reposts FOR SELECT USING (TRUE);

CREATE POLICY "reposts_insert_own"
    ON public.reposts FOR INSERT WITH CHECK (auth.uid() = user_id);

CREATE POLICY "reposts_delete_own"
    ON public.reposts FOR DELETE USING (auth.uid() = user_id);

-- ─── stories policies ─────────────────────────────────────────────────────────
CREATE POLICY "stories_select_active"
    ON public.stories FOR SELECT USING (expires_at > NOW());

CREATE POLICY "stories_insert_own"
    ON public.stories FOR INSERT WITH CHECK (auth.uid() = user_id);

CREATE POLICY "stories_delete_own"
    ON public.stories FOR DELETE USING (auth.uid() = user_id);

-- ─── story_views policies ─────────────────────────────────────────────────────
CREATE POLICY "story_views_select_all"
    ON public.story_views FOR SELECT USING (TRUE);

CREATE POLICY "story_views_insert_own"
    ON public.story_views FOR INSERT WITH CHECK (auth.uid() = viewer_id);

-- ─── conversations policies ───────────────────────────────────────────────────
CREATE POLICY "conversations_select_participant"
    ON public.conversations FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM public.conversation_participants cp
            WHERE cp.conversation_id = id
              AND cp.user_id = auth.uid()
        )
    );

CREATE POLICY "conversations_insert_authenticated"
    ON public.conversations FOR INSERT WITH CHECK (auth.uid() IS NOT NULL);

-- ─── conversation_participants policies ───────────────────────────────────────
CREATE POLICY "conv_participants_select_own"
    ON public.conversation_participants FOR SELECT USING (
        user_id = auth.uid() OR
        EXISTS (
            SELECT 1 FROM public.conversation_participants cp2
            WHERE cp2.conversation_id = conversation_id
              AND cp2.user_id = auth.uid()
        )
    );

CREATE POLICY "conv_participants_insert_authenticated"
    ON public.conversation_participants FOR INSERT WITH CHECK (auth.uid() IS NOT NULL);

-- ─── messages policies ────────────────────────────────────────────────────────
CREATE POLICY "messages_select_participant"
    ON public.messages FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM public.conversation_participants cp
            WHERE cp.conversation_id = messages.conversation_id
              AND cp.user_id = auth.uid()
        )
    );

CREATE POLICY "messages_insert_participant"
    ON public.messages FOR INSERT WITH CHECK (
        auth.uid() = sender_id AND
        EXISTS (
            SELECT 1 FROM public.conversation_participants cp
            WHERE cp.conversation_id = messages.conversation_id
              AND cp.user_id = auth.uid()
        )
    );

-- ─── channels policies ────────────────────────────────────────────────────────
CREATE POLICY "channels_select_public"
    ON public.channels FOR SELECT USING (
        is_public = TRUE OR
        EXISTS (
            SELECT 1 FROM public.channel_members cm
            WHERE cm.channel_id = id
              AND cm.user_id = auth.uid()
        )
    );

CREATE POLICY "channels_insert_authenticated"
    ON public.channels FOR INSERT WITH CHECK (auth.uid() = owner_id);

CREATE POLICY "channels_update_owner"
    ON public.channels FOR UPDATE USING (auth.uid() = owner_id);

-- ─── channel_members policies ─────────────────────────────────────────────────
CREATE POLICY "channel_members_select_all"
    ON public.channel_members FOR SELECT USING (TRUE);

CREATE POLICY "channel_members_insert_own"
    ON public.channel_members FOR INSERT WITH CHECK (auth.uid() = user_id);

CREATE POLICY "channel_members_delete_own"
    ON public.channel_members FOR DELETE USING (
        auth.uid() = user_id OR
        EXISTS (
            SELECT 1 FROM public.channel_members cm
            WHERE cm.channel_id = channel_id
              AND cm.user_id = auth.uid()
              AND cm.role IN ('owner', 'admin')
        )
    );

-- ─── channel_posts policies ───────────────────────────────────────────────────
CREATE POLICY "channel_posts_select_member"
    ON public.channel_posts FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM public.channels c
            LEFT JOIN public.channel_members cm ON cm.channel_id = c.id AND cm.user_id = auth.uid()
            WHERE c.id = channel_id
              AND (c.is_public = TRUE OR cm.user_id IS NOT NULL)
        )
    );

CREATE POLICY "channel_posts_insert_member"
    ON public.channel_posts FOR INSERT WITH CHECK (
        auth.uid() = sender_id AND
        EXISTS (
            SELECT 1 FROM public.channel_members cm
            WHERE cm.channel_id = channel_id
              AND cm.user_id = auth.uid()
        )
    );

-- ─── notifications policies ───────────────────────────────────────────────────
CREATE POLICY "notifications_select_own"
    ON public.notifications FOR SELECT USING (user_id = auth.uid());

CREATE POLICY "notifications_update_own"
    ON public.notifications FOR UPDATE USING (user_id = auth.uid());

-- =============================================================================
-- TRIGGER FUNCTIONS
-- =============================================================================

-- 1. handle_new_user — auto-create profile row on signup
CREATE OR REPLACE FUNCTION public.handle_new_user()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    _username TEXT;
BEGIN
    -- Derive username from email local-part + first 6 chars of UUID
    _username := LOWER(
        REGEXP_REPLACE(
            SPLIT_PART(NEW.email, '@', 1),
            '[^a-z0-9_]', '', 'g'
        )
    ) || '_' || LEFT(REPLACE(NEW.id::TEXT, '-', ''), 6);

    INSERT INTO public.users (id, username, display_name, created_at, updated_at)
    VALUES (
        NEW.id,
        _username,
        COALESCE(NEW.raw_user_meta_data->>'display_name', SPLIT_PART(NEW.email, '@', 1)),
        NOW(),
        NOW()
    )
    ON CONFLICT (id) DO NOTHING;

    RETURN NEW;
END;
$$;

CREATE TRIGGER on_auth_user_created
    AFTER INSERT ON auth.users
    FOR EACH ROW EXECUTE FUNCTION public.handle_new_user();

-- 2. update_follow_counts — keep follower/following counts in sync
CREATE OR REPLACE FUNCTION public.update_follow_counts()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE public.users SET following_count = following_count + 1 WHERE id = NEW.follower_id;
        UPDATE public.users SET follower_count  = follower_count  + 1 WHERE id = NEW.following_id;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE public.users SET following_count = GREATEST(following_count - 1, 0) WHERE id = OLD.follower_id;
        UPDATE public.users SET follower_count  = GREATEST(follower_count  - 1, 0) WHERE id = OLD.following_id;
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER trg_update_follow_counts
    AFTER INSERT OR DELETE ON public.follows
    FOR EACH ROW EXECUTE FUNCTION public.update_follow_counts();

-- 3. update_post_like_count — keep like_count on posts in sync
CREATE OR REPLACE FUNCTION public.update_post_like_count()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE public.posts SET like_count = like_count + 1 WHERE id = NEW.post_id;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE public.posts SET like_count = GREATEST(like_count - 1, 0) WHERE id = OLD.post_id;
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER trg_update_post_like_count
    AFTER INSERT OR DELETE ON public.likes
    FOR EACH ROW EXECUTE FUNCTION public.update_post_like_count();

-- 4. update_post_repost_count — keep repost_count on posts in sync
CREATE OR REPLACE FUNCTION public.update_post_repost_count()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE public.posts SET repost_count = repost_count + 1 WHERE id = NEW.post_id;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE public.posts SET repost_count = GREATEST(repost_count - 1, 0) WHERE id = OLD.post_id;
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER trg_update_post_repost_count
    AFTER INSERT OR DELETE ON public.reposts
    FOR EACH ROW EXECUTE FUNCTION public.update_post_repost_count();

-- 5. update_post_reply_count — keep reply_count on parent posts in sync
CREATE OR REPLACE FUNCTION public.update_post_reply_count()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
BEGIN
    IF TG_OP = 'INSERT' AND NEW.parent_id IS NOT NULL THEN
        UPDATE public.posts SET reply_count = reply_count + 1 WHERE id = NEW.parent_id;
    ELSIF TG_OP = 'DELETE' AND OLD.parent_id IS NOT NULL THEN
        UPDATE public.posts SET reply_count = GREATEST(reply_count - 1, 0) WHERE id = OLD.parent_id;
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER trg_update_post_reply_count
    AFTER INSERT OR DELETE ON public.posts
    FOR EACH ROW EXECUTE FUNCTION public.update_post_reply_count();

-- 6. update_updated_at — auto-update updated_at column
CREATE OR REPLACE FUNCTION public.update_updated_at_column()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_users_updated_at
    BEFORE UPDATE ON public.users
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();

CREATE TRIGGER trg_posts_updated_at
    BEFORE UPDATE ON public.posts
    FOR EACH ROW EXECUTE FUNCTION public.update_updated_at_column();
