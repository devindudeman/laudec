import { httpAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { internalMutation } from "./_generated/server";
import { v } from "convex/values";
import { Id } from "./_generated/dataModel";

// Simple hash — must match the one in apiKeys.ts
function simpleHash(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = (hash << 5) - hash + char;
    hash |= 0;
  }
  return hash.toString(36);
}

// ── HTTP Actions for laudec push ────────────────────────────────────

export const pushSession = httpAction(async (ctx, req) => {
  const teamId = await authenticateRequest(ctx, req);
  if (!teamId) {
    return new Response(JSON.stringify({ error: "Invalid API key" }), {
      status: 401,
      headers: { "Content-Type": "application/json" },
    });
  }

  const body = await req.json();
  const sessionId = await ctx.runMutation(internal.ingest.upsertSession, {
    teamId,
    session: body,
  });

  return new Response(JSON.stringify({ sessionId }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

export const pushCalls = httpAction(async (ctx, req) => {
  const teamId = await authenticateRequest(ctx, req);
  if (!teamId) {
    return new Response(JSON.stringify({ error: "Invalid API key" }), {
      status: 401,
      headers: { "Content-Type": "application/json" },
    });
  }

  const body = await req.json();
  await ctx.runMutation(internal.ingest.insertCalls, {
    teamId,
    sessionId: body.sessionId as Id<"sessions">,
    calls: body.calls,
  });

  return new Response(JSON.stringify({ ok: true }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

export const pushEvents = httpAction(async (ctx, req) => {
  const teamId = await authenticateRequest(ctx, req);
  if (!teamId) {
    return new Response(JSON.stringify({ error: "Invalid API key" }), {
      status: 401,
      headers: { "Content-Type": "application/json" },
    });
  }

  const body = await req.json();
  await ctx.runMutation(internal.ingest.insertEvents, {
    teamId,
    sessionId: body.sessionId as Id<"sessions">,
    events: body.events,
  });

  return new Response(JSON.stringify({ ok: true }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
});

// ── Internal mutations (called by HTTP actions) ─────────────────────

export const upsertSession = internalMutation({
  args: {
    teamId: v.id("teams"),
    session: v.object({
      runId: v.string(),
      ccSessionId: v.optional(v.string()),
      project: v.string(),
      projectPath: v.optional(v.string()),
      startedAt: v.string(),
      endedAt: v.optional(v.string()),
      durationSecs: v.optional(v.number()),
      apiCallCount: v.optional(v.number()),
      inputTokens: v.optional(v.number()),
      outputTokens: v.optional(v.number()),
      cacheRead: v.optional(v.number()),
      cacheWrite: v.optional(v.number()),
      costUsd: v.optional(v.number()),
      model: v.optional(v.string()),
      filesChanged: v.optional(v.number()),
      linesAdded: v.optional(v.number()),
      linesRemoved: v.optional(v.number()),
      changedFiles: v.optional(v.string()),
      summary: v.optional(v.string()),
      toolUses: v.optional(v.number()),
      firstPrompt: v.optional(v.string()),
      errorCount: v.optional(v.number()),
      machineId: v.optional(v.string()),
      status: v.union(v.literal("active"), v.literal("completed")),
    }),
  },
  returns: v.id("sessions"),
  handler: async (ctx, args) => {
    // Check if session already exists (by runId)
    const existing = await ctx.db
      .query("sessions")
      .withIndex("by_run_id", (q) => q.eq("runId", args.session.runId))
      .unique();

    if (existing) {
      await ctx.db.patch(existing._id, {
        ...args.session,
        teamId: args.teamId,
      });
      return existing._id;
    }

    return await ctx.db.insert("sessions", {
      teamId: args.teamId,
      ...args.session,
    });
  },
});

export const insertCalls = internalMutation({
  args: {
    teamId: v.id("teams"),
    sessionId: v.id("sessions"),
    calls: v.array(
      v.object({
        callId: v.string(),
        timestamp: v.string(),
        method: v.string(),
        path: v.string(),
        statusCode: v.optional(v.number()),
        latencyMs: v.optional(v.number()),
        model: v.optional(v.string()),
        inputTokens: v.optional(v.number()),
        outputTokens: v.optional(v.number()),
        cacheRead: v.optional(v.number()),
        cacheWrite: v.optional(v.number()),
        responseText: v.optional(v.string()),
        requestBody: v.optional(v.string()),
        responseBody: v.optional(v.string()),
        requestHeaders: v.optional(v.string()),
        responseHeaders: v.optional(v.string()),
      })
    ),
  },
  returns: v.null(),
  handler: async (ctx, args) => {
    for (const call of args.calls) {
      await ctx.db.insert("apiCalls", {
        teamId: args.teamId,
        sessionId: args.sessionId,
        ...call,
      });
    }
    return null;
  },
});

export const insertEvents = internalMutation({
  args: {
    teamId: v.id("teams"),
    sessionId: v.id("sessions"),
    events: v.array(
      v.object({
        timestamp: v.string(),
        name: v.string(),
        body: v.optional(v.string()),
        attributes: v.optional(v.string()),
        severity: v.optional(v.string()),
      })
    ),
  },
  returns: v.null(),
  handler: async (ctx, args) => {
    for (const event of args.events) {
      await ctx.db.insert("otelEvents", {
        teamId: args.teamId,
        sessionId: args.sessionId,
        ...event,
      });
    }
    return null;
  },
});

// ── Auth helper ─────────────────────────────────────────────────────

async function authenticateRequest(
  ctx: { runQuery: typeof import("./_generated/server").internalQuery },
  req: Request
): Promise<Id<"teams"> | null> {
  const authHeader = req.headers.get("Authorization");
  if (!authHeader?.startsWith("Bearer ")) return null;

  const apiKey = authHeader.slice(7);
  const keyHash = simpleHash(apiKey);

  const teamId: Id<"teams"> | null = await (ctx as any).runQuery(
    internal.apiKeys.validateKey,
    { keyHash }
  );
  return teamId;
}
