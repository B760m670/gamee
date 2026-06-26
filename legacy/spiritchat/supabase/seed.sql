-- =============================================================================
-- Seed Data for MySocialApp
-- NOTE: These UUIDs simulate auth.users entries. In a real Supabase project,
--       insert into auth.users first or use the handle_new_user trigger.
-- =============================================================================

-- ─── Users ────────────────────────────────────────────────────────────────────
INSERT INTO public.users (id, username, display_name, bio, avatar_url, is_verified)
VALUES
    ('00000000-0000-0000-0000-000000000001',
     'alice_dev',
     'Alice',
     'Building cool things on the internet.',
     'https://i.pravatar.cc/150?u=alice',
     TRUE),
    ('00000000-0000-0000-0000-000000000002',
     'bob_codes',
     'Bob',
     'Software engineer & coffee enthusiast.',
     'https://i.pravatar.cc/150?u=bob',
     FALSE),
    ('00000000-0000-0000-0000-000000000003',
     'carol_ux',
     'Carol',
     'Designer who loves clean interfaces.',
     'https://i.pravatar.cc/150?u=carol',
     FALSE)
ON CONFLICT (id) DO NOTHING;

-- ─── Follows ──────────────────────────────────────────────────────────────────
INSERT INTO public.follows (follower_id, following_id)
VALUES
    ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000001'),
    ('00000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000001'),
    ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000002'),
    ('00000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000002')
ON CONFLICT DO NOTHING;

-- Update counts manually since auth triggers won't fire for seed data
UPDATE public.users SET follower_count = 2, following_count = 1 WHERE id = '00000000-0000-0000-0000-000000000001';
UPDATE public.users SET follower_count = 2, following_count = 1 WHERE id = '00000000-0000-0000-0000-000000000002';
UPDATE public.users SET follower_count = 0, following_count = 2 WHERE id = '00000000-0000-0000-0000-000000000003';

-- ─── Posts ────────────────────────────────────────────────────────────────────
INSERT INTO public.posts (id, author_id, content)
VALUES
    ('10000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001',
     'Just shipped a new feature for the social app! Threads + Telegram + TikTok vibes all in one.'),
    ('10000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000002',
     'Really enjoying the new monorepo setup with Turborepo. Build times are so much faster.'),
    ('10000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000001',
     'Hot take: short-form video is still the highest engagement format in 2025.'),
    ('10000000-0000-0000-0000-000000000004', '00000000-0000-0000-0000-000000000003',
     'Designed a new onboarding flow today. Reduced steps from 7 to 3. Users will love it.'),
    ('10000000-0000-0000-0000-000000000005', '00000000-0000-0000-0000-000000000002',
     'Go + Fiber for the backend is such an underrated combo. Blazing fast, minimal memory.'),
    ('10000000-0000-0000-0000-000000000006', '00000000-0000-0000-0000-000000000001',
     'Supabase RLS is genuinely impressive once you wrap your head around it.'),
    ('10000000-0000-0000-0000-000000000007', '00000000-0000-0000-0000-000000000003',
     'Dark mode first design is the way to go for social apps. Less eye strain during late-night scrolling.'),
    ('10000000-0000-0000-0000-000000000008', '00000000-0000-0000-0000-000000000002',
     'Wrote a Redis fan-out service for the feed today. Each user gets their own sorted set.'),
    ('10000000-0000-0000-0000-000000000009', '00000000-0000-0000-0000-000000000001',
     'FlashList from Shopify is genuinely so much better than FlatList for long feeds.'),
    ('10000000-0000-0000-0000-000000000010', '00000000-0000-0000-0000-000000000003',
     'NativeWind + Expo Router is the combo I did not know I needed. Highly recommend.')
ON CONFLICT (id) DO NOTHING;

-- Reply to post 1
INSERT INTO public.posts (id, author_id, content, parent_id, root_id)
VALUES
    ('10000000-0000-0000-0000-000000000011', '00000000-0000-0000-0000-000000000002',
     'This looks awesome! What is the stack?',
     '10000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001')
ON CONFLICT (id) DO NOTHING;

-- ─── Likes ────────────────────────────────────────────────────────000000000────
INSERT INTO public.likes (user_id, post_id)
VALUES
    ('00000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000001'),
    ('00000000-0000-0000-0000-000000000003', '10000000-0000-0000-0000-000000000001'),
    ('00000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000002'),
    ('00000000-0000-0000-0000-000000000003', '10000000-0000-0000-0000-000000000005'),
    ('00000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000008')
ON CONFLICT DO NOTHING;

-- Update like counts to match seed likes
UPDATE public.posts SET like_count = 2 WHERE id = '10000000-0000-0000-0000-000000000001';
UPDATE public.posts SET like_count = 1 WHERE id = '10000000-0000-0000-0000-000000000002';
UPDATE public.posts SET like_count = 1 WHERE id = '10000000-0000-0000-0000-000000000005';
UPDATE public.posts SET like_count = 1 WHERE id = '10000000-0000-0000-0000-000000000008';

-- Update reply count on post 1
UPDATE public.posts SET reply_count = 1 WHERE id = '10000000-0000-0000-0000-000000000001';

-- Update post counts for users
UPDATE public.users SET post_count = 4 WHERE id = '00000000-0000-0000-0000-000000000001';
UPDATE public.users SET post_count = 3 WHERE id = '00000000-0000-0000-0000-000000000002';
UPDATE public.users SET post_count = 3 WHERE id = '00000000-0000-0000-0000-000000000003';
