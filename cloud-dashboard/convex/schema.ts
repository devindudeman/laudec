import { defineSchema, defineTable } from "convex/server";
import { authTables } from "@convex-dev/auth/server";
import { v } from "convex/values";

export default defineSchema({
  ...authTables,

  // ── Teams ─────────────────────────────────────────────────────────
  teams: defineTable({
    name: v.string(),
    ownerId: v.id("users"),
    createdAt: v.number(),
  }).index("by_owner", ["ownerId"]),

  teamMembers: defineTable({
    teamId: v.id("teams"),
    userId: v.id("users"),
    role: v.union(v.literal("owner"), v.literal("admin"), v.literal("member")),
    joinedAt: v.number(),
  })
    .index("by_team", ["teamId"])
    .index("by_user", ["userId"])
    .index("by_team_and_user", ["teamId", "userId"]),

  // ── API Keys (for laudec push) ────────────────────────────────────
  apiKeys: defineTable({
    teamId: v.id("teams"),
    name: v.string(),
    keyHash: v.string(),
    keyPrefix: v.string(),
    createdBy: v.id("users"),
    createdAt: v.number(),
    lastUsedAt: v.optional(v.number()),
    revokedAt: v.optional(v.number()),
  })
    .index("by_team", ["teamId"])
    .index("by_key_hash", ["keyHash"]),

  // ── Sessions (laudec sessions pushed from CLI) ────────────────────
  sessions: defineTable({
    teamId: v.id("teams"),
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
  })
    .index("by_team", ["teamId"])
    .index("by_team_and_started", ["teamId", "startedAt"])
    .index("by_run_id", ["runId"]),

  // ── API Calls (proxy log entries) ─────────────────────────────────
  apiCalls: defineTable({
    teamId: v.id("teams"),
    sessionId: v.id("sessions"),
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
    // Request/response bodies stored as strings (can be large)
    requestBody: v.optional(v.string()),
    responseBody: v.optional(v.string()),
    requestHeaders: v.optional(v.string()),
    responseHeaders: v.optional(v.string()),
  })
    .index("by_session", ["sessionId"])
    .index("by_team_and_timestamp", ["teamId", "timestamp"]),

  // ── OTEL Events ───────────────────────────────────────────────────
  otelEvents: defineTable({
    teamId: v.id("teams"),
    sessionId: v.id("sessions"),
    timestamp: v.string(),
    name: v.string(),
    body: v.optional(v.string()),
    attributes: v.optional(v.string()),
    severity: v.optional(v.string()),
  })
    .index("by_session", ["sessionId"])
    .index("by_session_and_name", ["sessionId", "name"])
    .index("by_team_and_timestamp", ["teamId", "timestamp"]),
});
