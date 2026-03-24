# laudec

See everything Claude Code does. One binary. One command.

laudec wraps Claude Code with a transparent proxy, OTEL collector, and web dashboard — giving you full visibility into every API call, token, tool use, and prompt that flows through a session.

## Install

**One-liner** (requires git, Rust, and Node.js):

```bash
curl -fsSL https://raw.githubusercontent.com/devindudeman/laudec/main/install.sh | bash
```

**Or manually:**

```bash
git clone https://github.com/devindudeman/laudec.git
cd laudec
cd dashboard && npm install && npm run build && cd ..
cargo install --path .
```

Works on **Linux** and **macOS**. Requires [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed.

## Quick Start

```bash
cd your-project/
laudec .
```

That's it. laudec launches Claude Code with instrumentation wired up automatically. Open the dashboard URL printed at startup.

## What You Get

### Proxy Tab
Full API traffic inspection for every request to Anthropic:
- Calls classified by type (MAIN, SUBAGENT, QUOTA, TOKEN COUNT) with color-coded pills
- MAIN calls labeled by turn number or tool loop
- Subagent calls tagged with role (EXPLORE, WEB SEARCH, CC GUIDE)
- User query and model response rendered as markdown, collapsible per card
- Tool usage summary per call (e.g. `Read x3 · Edit x2`)
- Error calls (status >= 400) highlighted with red card tint
- System-injected blocks (system reminders, tool rules) in collapsible sections
- Request/response bodies with JSON syntax highlighting
- Token counts, cache, latency, model, and status per call

### Events Tab
OTEL telemetry grouped by conversation turn:
- User prompts, API requests, tool decisions, and tool results
- Per-turn token/cost breakdown
- Tool parameters and error details

### Metrics Tab
Session-level summary:
- Total cost, API call count, average latency, cache hit rate
- Token breakdown (input / output / cache) as a stacked bar
- Per-call table with totals
- Tool usage with success/failure counts

### Insights Tab
Derived analytics from proxy headers and response bodies:
- **Cache analysis** — hit rate, estimated cost savings, cache read/write totals
- **Stop reasons** — breakdown of end_turn, tool_use, max_tokens across all calls
- **System prompt size** — estimated token count of your system prompt
- **Context growth** — input tokens per call showing context window fill-up over time
- **Rate limits** — remaining requests/tokens from Anthropic response headers, with warning thresholds

## How It Works

```
Claude Code  -->  laudec proxy  -->  Anthropic API
     |                 |
     |                 v
     |           SQLite (api_calls)
     |
     +-->  laudec OTEL collector (gRPC)
                       |
                       v
                 SQLite (otel_events, otel_metrics)
```

laudec sits between Claude Code and Anthropic's API:

1. **Proxy** — An HTTP proxy that intercepts all API calls, logs request/response bodies, parses token usage, and forwards to Anthropic
2. **OTEL Collector** — A gRPC server that receives OpenTelemetry logs and metrics from Claude Code (prompts, tool usage, costs)
3. **Dashboard** — A Svelte SPA served by the same binary, querying the SQLite database

All data is stored locally in `~/.local/share/laudec/laudec.db`.

## CLI

```bash
laudec .                        # Run Claude Code with full instrumentation
laudec . -p "fix the tests"    # Single prompt (non-interactive)
laudec dashboard                # Start dashboard without launching Claude Code
laudec log                      # View session history
laudec log --all                # All projects
laudec calls --session ID       # View API calls for a session
laudec events --session ID      # View OTEL events
laudec replay SESSION_ID        # Replay a session timeline
laudec stats                    # Show usage statistics
laudec init                     # Generate starter laudec.toml
laudec config                   # Print resolved configuration
```

## Configuration

Generate a starter config:

```bash
laudec init
```

This creates `laudec.toml` in your project directory. Config is also loaded from `~/.config/laudec/config.toml` for global defaults.

### Key options

```toml
[proxy]
enabled = true
port = 18080
log_requests = true       # Log request bodies
log_responses = true      # Log response bodies
redact_keys = true        # Redact API keys in stored headers

[telemetry]
enabled = true
collector_port = 14317
log_prompts = true        # Store user prompts from OTEL
log_tool_details = true   # Store tool parameters/results

[dashboard]
enabled = true
port = 18384

[sandbox]
enabled = true
allowed_domains = []      # Additional domains beyond api.anthropic.com
allow_write = false

[permissions]
mode = "plan"             # "plan", "auto", or "bypassPermissions"
allow = []                # Tool allow list
deny = []                 # Tool deny list

[claude]
# model = "claude-sonnet-4-6"  # Override default model

[cloud]
# Push session data to a centralized cloud dashboard
# enabled = true
# endpoint = "https://your-project.convex.site"
# api_key = "ldc_..."
# push_bodies = true  # Set false to skip request/response bodies
```

### Remote mode

Point proxy or collector at an existing instance:

```toml
[proxy]
remote = "http://shared-proxy:18080"

[telemetry]
remote = "http://shared-collector:14317"
```

## Cloud Dashboard

laudec can push session data to a centralized cloud dashboard powered by [Convex](https://www.convex.dev/). This gives you a shared, multi-user, real-time view of all Claude Code sessions across your team.

Features:
- **Multi-tenant** — sign in with GitHub or Google, create teams, invite members
- **Live updates** — sessions appear in the dashboard as they happen (powered by Convex's reactive queries)
- **Full proxy inspection** — same call classification (MAIN/SUBAGENT/QUOTA), conversation view (YOU/MODEL), and raw request/response bodies as the local dashboard
- **OTEL events** — prompts, API requests, tool decisions/results grouped by turn
- **Metrics** — cost, token breakdown, latency, cache hit rate
- **API keys** — per-team keys for authenticating laudec instances
- **Offline-safe** — data always goes to local SQLite first; cloud push is best-effort and non-blocking

### Quick Start

1. **Sign in** at your cloud dashboard URL and create a team
2. **Create an API key** in Settings → copy the `ldc_...` key
3. **Configure laudec** — add to `~/.config/laudec/config.toml` (global) or `laudec.toml` (per-project):

```toml
[cloud]
enabled = true
endpoint = "https://your-project.convex.site"
api_key = "ldc_your_key_here"
```

4. **Run laudec** — you'll see `cloud push enabled` in the banner:

```
laudec .
```

5. **Check the dashboard** — your session appears in real-time with full drill-down

### What gets pushed

By default, laudec pushes:

- **Session metadata** — project, duration, tokens, cost, model, files changed, summary
- **API call log** — timestamps, models, token counts, latency, cache stats, response text, full request/response bodies and headers
- **OTEL events** — user prompts, API requests, tool decisions, tool results with attributes

The proxy tab's call classifier (MAIN/SUBAGENT/QUOTA/TOKEN COUNT) and conversation view (YOU/MODEL) require request bodies to work. If you want to save bandwidth and only see session summaries, set `push_bodies = false`:

```toml
[cloud]
enabled = true
endpoint = "https://your-project.convex.site"
api_key = "ldc_..."
push_bodies = false  # Only push metadata, not full request/response bodies
```

### Deploying the Cloud Dashboard

The cloud dashboard is a Next.js app backed by Convex. To deploy your own:

```bash
cd cloud-dashboard

# Install dependencies
npm install

# Set up Convex (creates a new project)
npx convex dev --once

# Generate auth keys
node generateKeys.mjs
# Copy the output (JWT_PRIVATE_KEY and JWKS) to Convex env vars

# Set required environment variables in the Convex dashboard:
# - SITE_URL: your frontend URL (e.g. https://your-app.vercel.app)
# - JWT_PRIVATE_KEY: from generateKeys.mjs
# - JWKS: from generateKeys.mjs
# - AUTH_GITHUB_ID / AUTH_GITHUB_SECRET: from a GitHub OAuth App
# - AUTH_GOOGLE_ID / AUTH_GOOGLE_SECRET: (optional) from Google Cloud Console

# For the GitHub OAuth App, set:
#   Homepage URL: https://your-project.convex.site
#   Callback URL: https://your-project.convex.site/api/auth/callback/github

# Run the frontend locally
npm run dev

# Or deploy to Vercel
npx vercel
```

### Architecture

```
laudec (local)                    Convex (cloud)                 Next.js Dashboard
─────────────                    ──────────────                 ─────────────────
proxy + collector                                               
      │                                                         
      ├─ POST /api/ingest/* ──▶  HTTP Actions (auth + ingest)  React + Tailwind
      │   session, calls,         ├─ validate API key           ├─ GitHub/Google auth
      │   otel events             ├─ strip nulls                ├─ real-time sessions
      │                           └─ write to Convex DB         ├─ proxy/events/metrics
      └─ local SQLite                                           └─ team/key management
         (always written          Convex DB                           ▲
          first, works            ├─ users (OAuth)                    │
          offline)                ├─ teams + members            useQuery() reactive
                                  ├─ apiKeys                    subscriptions via
                                  ├─ sessions                   WebSocket
                                  ├─ apiCalls                         │
                                  └─ otelEvents                       │
                                                                Convex ◀─── WebSocket
```

## Building from Source

Requirements:
- Rust toolchain (1.75+)
- Node.js (18+)

```bash
# Build the dashboard frontend first
cd dashboard && npm install && npm run build && cd ..

# Build the binary (embeds the dashboard)
cargo build --release

# Binary is at target/release/laudec
```

## Data

All data lives in a single SQLite database:

```bash
# Default location
~/.local/share/laudec/laudec.db

# Override with environment variable
export LAUDEC_DATA_DIR=/path/to/data
```

Inspect directly:

```bash
sqlite3 ~/.local/share/laudec/laudec.db "SELECT * FROM sessions ORDER BY started_at DESC LIMIT 5"
sqlite3 ~/.local/share/laudec/laudec.db "SELECT * FROM session_id_map"
```

## License

MIT
