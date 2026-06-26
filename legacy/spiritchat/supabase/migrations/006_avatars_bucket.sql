-- Create the avatars storage bucket (public, 5 MB limit)
INSERT INTO storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
VALUES (
  'avatars',
  'avatars',
  true,
  5242880,
  ARRAY['image/jpeg','image/png','image/webp','image/gif']
)
ON CONFLICT (id) DO NOTHING;

-- Public read
CREATE POLICY IF NOT EXISTS "avatars_select_public"
  ON storage.objects FOR SELECT
  USING (bucket_id = 'avatars');

-- Authenticated upload
CREATE POLICY IF NOT EXISTS "avatars_insert_auth"
  ON storage.objects FOR INSERT
  WITH CHECK (bucket_id = 'avatars' AND (SELECT auth.uid()) IS NOT NULL);

-- Owner update
CREATE POLICY IF NOT EXISTS "avatars_update_own"
  ON storage.objects FOR UPDATE
  USING (bucket_id = 'avatars' AND owner_id = (SELECT auth.uid())::text);

-- Owner delete
CREATE POLICY IF NOT EXISTS "avatars_delete_own"
  ON storage.objects FOR DELETE
  USING (bucket_id = 'avatars' AND owner_id = (SELECT auth.uid())::text);
