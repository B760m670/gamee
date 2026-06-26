# MySocialApp

A social media platform combining the best of Threads, Telegram, and TikTok. Built as a Turborepo monorepo with a Go/Fiber API, React Native/Expo mobile app, and Next.js 14 web app.

## Repository structure

```
mysocialapp/
  apps/
    api/          Go + Fiber backend
    mobile/       React Native + Expo (Expo Router)
    web/          Next.js 14 (App Router)
  packages/
    types/        Shared TypeScript types
  supabase/
    migrations/   PostgreSQL schema (Supabase)
    seed.sql      Test data
```

## Prerequisites

- Node.js 20+
- Go 1.22+
- Expo CLI (`npm i -g expo-cli`)
- A Supabase project
- An Upstash Redis instance (optional; API falls back to Postgres)

## Getting started

### 1. Install dependencies

```bash
npm install
```

### 2. Environment variables

Copy the example files and fill in your values:

```bash
cp apps/api/.env.example apps/api/.env
cp apps/mobile/.env.example apps/mobile/.env.local
cp apps/web/.env.example apps/web/.env.local
```

### 3. Run the database migrations

In the Supabase dashboard SQL editor, run:

```
supabase/migrations/001_initial_schema.sql
supabase/seed.sql          # optional test data
```

### 4. Start all apps

```bash
npm run dev
```

This uses Turborepo to run all `dev` scripts in parallel.

## Running apps individually

### Go API

```bash
cd apps/api
go run ./cmd/server
```

Runs on `http://localhost:8080`. Health check: `GET /health`.

### Expo mobile

```bash
cd apps/mobile
npx expo start
```

Press `i` for iOS simulator, `a` for Android emulator, or scan the QR code with Expo Go.

### Next.js web

```bash
cd apps/web
npm run dev
```

Runs on `http://localhost:3000`.

## API overview

All routes are under `/api/v1`. Protected routes require `Authorization: Bearer <token>`.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | /health | - | Health check |
| GET | /api/v1/feed | required | Paginated home feed |
| POST | /api/v1/posts | required | Create post |
| GET | /api/v1/posts/:id | - | Get post |
| POST | /api/v1/posts/:id/like | required | Toggle like |
| GET | /api/v1/posts/:id/replies | - | Get replies |
| GET | /api/v1/users/me | required | Get own profile |
| PUT | /api/v1/users/me | required | Update profile |
| GET | /api/v1/users/:username | - | Get user by username |
| POST | /api/v1/users/:id/follow | required | Follow user |
| DELETE | /api/v1/users/:id/follow | required | Unfollow user |
| GET | /api/v1/users/:id/followers | - | Get followers |
| GET | /api/v1/users/:id/following | - | Get following |

## Architecture

- **Auth**: Supabase Auth (email + password). JWTs are passed as Bearer tokens to the Go API, which validates them using the project JWT secret.
- **Database**: Supabase (PostgreSQL). Row-level security enforced on all tables.
- **Feed**: Fan-out-on-write via Redis sorted sets (score = Unix timestamp ms). Falls back to Postgres on cache miss.
- **Real-time**: WebSocket hub in Go for push notifications and live message delivery.
- **Mobile**: Zustand for auth state, TanStack Query for server state, NativeWind for styling.
- **Web**: Next.js server components for initial data fetch, TanStack Query for client-side revalidation.
