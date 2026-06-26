-- Migration 013: rename phone → email for email-based OTP auth.
-- Run once in the Supabase SQL editor.

ALTER TABLE users RENAME COLUMN phone TO email;

-- Rename the unique constraint index
ALTER INDEX IF EXISTS idx_users_phone RENAME TO idx_users_email;
