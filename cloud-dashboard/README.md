# laudec cloud dashboard

Centralized, multi-tenant dashboard for Claude Code sessions. Powered by [Convex](https://www.convex.dev/) and Next.js.

## Stack

- **Backend**: [Convex](https://www.convex.dev/) — reactive database, TypeScript server functions, real-time subscriptions
- **Frontend**: Next.js 16 + React + Tailwind CSS
- **Auth**: GitHub + Google OAuth via [@convex-dev/auth](https://labs.convex.dev/auth)

## Setup

### 1. Install dependencies

```bash
npm install
```

### 2. Set up Convex

```bash
npx convex dev --once
```

This creates a Convex project and writes URLs to `.env.local`.

### 3. Generate auth keys

```bash
node generateKeys.mjs
```

Copy the output (`JWT_PRIVATE_KEY` and `JWKS`) to your Convex deployment's environment variables.

### 4. Set environment variables

In the [Convex dashboard](https://dashboard.convex.dev/) → Settings → Environment Variables:

| Variable | Description |
|----------|-------------|
| `SITE_URL` | Your frontend URL (e.g. `http://localhost:3000` for dev) |
| `JWT_PRIVATE_KEY` | From `generateKeys.mjs` |
| `JWKS` | From `generateKeys.mjs` |
| `AUTH_GITHUB_ID` | GitHub OAuth App client ID |
| `AUTH_GITHUB_SECRET` | GitHub OAuth App client secret |
| `AUTH_GOOGLE_ID` | (Optional) Google OAuth client ID |
| `AUTH_GOOGLE_SECRET` | (Optional) Google OAuth client secret |

For the **GitHub OAuth App**:
- Homepage URL: `https://your-project.convex.site`
- Authorization callback URL: `https://your-project.convex.site/api/auth/callback/github`

### 5. Run

```bash
npm run dev
```

Open http://localhost:3000 → Sign in with GitHub → Create a team → Create an API key.

## Convex Schema

| Table | Purpose |
|-------|---------|
| `users` | OAuth users (managed by @convex-dev/auth) |
| `teams` | Multi-tenant teams |
| `teamMembers` | Team membership with roles (owner/admin/member) |
| `apiKeys` | Per-team API keys for laudec auth |
| `sessions` | Claude Code sessions pushed from laudec |
| `apiCalls` | Proxy log entries (API calls to Anthropic) |
| `otelEvents` | OTEL telemetry (prompts, tool decisions, tool results) |

## Ingest API

laudec pushes data to these HTTP endpoints (authenticated with Bearer token):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/ingest/session` | POST | Create or update a session |
| `/api/ingest/calls` | POST | Push a batch of API call records |
| `/api/ingest/events` | POST | Push a batch of OTEL events |

## Dashboard Pages

- `/` — Sessions list (table with time, duration, project, model, calls, cost)
- `/session/[id]` — Session detail with Proxy, Events, and Metrics tabs
- `/settings` — Team info, API key management, setup guide

## Deploying

The frontend can be deployed to Vercel, Netlify, or any static hosting:

```bash
npx vercel
```

The Convex backend is already in the cloud — no additional deployment needed.
