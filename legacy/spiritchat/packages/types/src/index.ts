// ─── Enums ────────────────────────────────────────────────────────────────────

export enum MediaType {
  Image = 'image',
  Video = 'video',
}

export enum ConversationType {
  Direct = 'direct',
  Group = 'group',
}

export enum NotificationType {
  Like = 'like',
  Follow = 'follow',
  Reply = 'reply',
  Mention = 'mention',
  DM = 'dm',
  Repost = 'repost',
  ChannelPost = 'channel_post',
}

export enum ChannelMemberRole {
  Owner = 'owner',
  Admin = 'admin',
  Member = 'member',
}

// ─── User & Profile ───────────────────────────────────────────────────────────

export interface User {
  id: string
  username: string
  display_name: string
  bio: string | null
  avatar_url: string | null
  website: string | null
  follower_count: number
  following_count: number
  post_count: number
  is_verified: boolean
  created_at: string
  updated_at: string
}

export interface Profile extends User {
  is_following?: boolean
  is_followed_by?: boolean
}

export interface Follow {
  follower_id: string
  following_id: string
  created_at: string
  follower?: User
  following?: User
}

// ─── Media ────────────────────────────────────────────────────────────────────

export interface PostMedia {
  id: string
  uploader_id: string
  url: string
  thumbnail_url: string | null
  type: MediaType
  duration_sec: number | null
  width: number | null
  height: number | null
  size_bytes: number | null
  created_at: string
}

// ─── Posts ────────────────────────────────────────────────────────────────────

export interface Post {
  id: string
  author_id: string
  content: string
  parent_id: string | null
  root_id: string | null
  media_ids: string[]
  like_count: number
  reply_count: number
  repost_count: number
  view_count: number
  is_deleted: boolean
  created_at: string
  updated_at: string
  author?: User
  media?: PostMedia[]
  liked_by_me?: boolean
  reposted_by_me?: boolean
}

export interface Like {
  user_id: string
  post_id: string
  created_at: string
}

export interface Repost {
  user_id: string
  post_id: string
  created_at: string
}

// ─── Stories ──────────────────────────────────────────────────────────────────

export interface Story {
  id: string
  user_id: string
  media_id: string
  type: MediaType
  expires_at: string
  view_count: number
  created_at: string
  user?: User
  media?: PostMedia
  viewed_by_me?: boolean
}

export interface StoryView {
  story_id: string
  viewer_id: string
  viewed_at: string
}

// ─── Conversations & Messages ─────────────────────────────────────────────────

export interface Conversation {
  id: string
  type: ConversationType
  name: string | null
  avatar_url: string | null
  last_message_at: string | null
  created_at: string
  participants?: ConversationParticipant[]
  last_message?: Message
  unread_count?: number
}

export interface ConversationParticipant {
  conversation_id: string
  user_id: string
  last_read_at: string | null
  joined_at: string
  user?: User
}

export interface Message {
  id: string
  conversation_id: string
  sender_id: string
  content: string | null
  media_url: string | null
  reply_to_id: string | null
  is_deleted: boolean
  created_at: string
  sender?: User
  reply_to?: Message
}

// ─── Channels ─────────────────────────────────────────────────────────────────

export interface Channel {
  id: string
  name: string
  handle: string
  description: string | null
  avatar_url: string | null
  owner_id: string
  member_count: number
  is_public: boolean
  created_at: string
  owner?: User
  member_role?: ChannelMemberRole
}

export interface ChannelMember {
  channel_id: string
  user_id: string
  role: ChannelMemberRole
  joined_at: string
  user?: User
}

export interface ChannelPost {
  id: string
  channel_id: string
  sender_id: string
  content: string | null
  media_url: string | null
  created_at: string
  sender?: User
  channel?: Channel
}

// ─── Notifications ────────────────────────────────────────────────────────────

export interface Notification {
  id: string
  user_id: string
  type: NotificationType
  actor_id: string | null
  post_id: string | null
  message_id: string | null
  is_read: boolean
  created_at: string
  actor?: User
  post?: Post
  message?: Message
}

// ─── Feed ─────────────────────────────────────────────────────────────────────

export type FeedItem = Post & {
  reposted_by?: User
}

export interface FeedPost {
  id: string
  author_id: string
  content: string
  media_url: string | null
  media_ids: string[]
  like_count: number
  reply_count: number
  repost_count: number
  view_count: number
  parent_id: string | null
  root_id: string | null
  created_at: string
  updated_at: string
  author_username: string
  author_display_name: string
  author_avatar_url: string | null
  author_is_verified: boolean
  liked_by_me: boolean
  reposted_by_me: boolean
  bookmarked_by_me: boolean
}

export interface FeedResponse {
  posts: FeedPost[]
  next_cursor: string | null
  has_more: boolean
}

export interface CreatePostInput {
  content: string
  media_url?: string
  parent_id?: string
}

// ─── Pagination & API ─────────────────────────────────────────────────────────

export interface PaginatedResponse<T> {
  data: T[]
  next_cursor: string | null
  has_more: boolean
  total?: number
}

export interface ApiResponse<T> {
  data: T
  message?: string
}

export interface ApiError {
  message: string
  code?: string
  status: number
  details?: Record<string, string[]>
}

// ─── Auth ─────────────────────────────────────────────────────────────────────

export interface AuthSession {
  access_token: string
  refresh_token: string
  expires_at: number
  user: User
}

export interface LoginRequest {
  email: string
  password: string
}

export interface RegisterRequest {
  username: string
  email: string
  password: string
}

// ─── Create / Update DTOs ─────────────────────────────────────────────────────

export interface CreatePostRequest {
  content: string
  parent_id?: string
  media_ids?: string[]
}

export interface UpdateProfileRequest {
  display_name?: string
  bio?: string
  avatar_url?: string
  website?: string
}

export interface CreateMessageRequest {
  content?: string
  media_url?: string
  reply_to_id?: string
}

export interface CreateChannelRequest {
  name: string
  handle: string
  description?: string
  is_public?: boolean
}
