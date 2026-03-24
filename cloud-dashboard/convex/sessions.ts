import { query } from "./_generated/server";
import { v } from "convex/values";
import { getAuthUserId } from "@convex-dev/auth/server";
import { Id } from "./_generated/dataModel";

// Helper: get team IDs for the current user
async function getUserTeamIds(ctx: any): Promise<Id<"teams">[]> {
  const userId = await getAuthUserId(ctx);
  if (!userId) return [];

  const memberships = await ctx.db
    .query("teamMembers")
    .withIndex("by_user", (q: any) => q.eq("userId", userId))
    .collect();

  return memberships.map((m: any) => m.teamId);
}

export const list = query({
  args: {
    teamId: v.id("teams"),
    limit: v.optional(v.number()),
  },
  returns: v.array(
    v.object({
      _id: v.id("sessions"),
      _creationTime: v.number(),
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
  ),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) return [];

    // Verify membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", args.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) return [];

    const limit = args.limit ?? 50;
    return await ctx.db
      .query("sessions")
      .withIndex("by_team_and_started", (q) => q.eq("teamId", args.teamId))
      .order("desc")
      .take(limit);
  },
});

export const get = query({
  args: { sessionId: v.id("sessions") },
  returns: v.union(
    v.object({
      _id: v.id("sessions"),
      _creationTime: v.number(),
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
    }),
    v.null()
  ),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) return null;

    const session = await ctx.db.get(args.sessionId);
    if (!session) return null;

    // Verify team membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", session.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) return null;

    return session;
  },
});

export const getCalls = query({
  args: {
    sessionId: v.id("sessions"),
    limit: v.optional(v.number()),
  },
  returns: v.array(
    v.object({
      _id: v.id("apiCalls"),
      _creationTime: v.number(),
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
      callType: v.optional(v.string()),
      callDetail: v.optional(v.string()),
      toolTags: v.optional(v.string()),
      userQuery: v.optional(v.string()),
      requestBody: v.optional(v.string()),
      responseBody: v.optional(v.string()),
      requestHeaders: v.optional(v.string()),
      responseHeaders: v.optional(v.string()),
    })
  ),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) return [];

    const session = await ctx.db.get(args.sessionId);
    if (!session) return [];

    // Verify team membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", session.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) return [];

    const limit = args.limit ?? 200;
    return await ctx.db
      .query("apiCalls")
      .withIndex("by_session", (q) => q.eq("sessionId", args.sessionId))
      .take(limit);
  },
});

export const getEvents = query({
  args: {
    sessionId: v.id("sessions"),
    limit: v.optional(v.number()),
  },
  returns: v.array(
    v.object({
      _id: v.id("otelEvents"),
      _creationTime: v.number(),
      teamId: v.id("teams"),
      sessionId: v.id("sessions"),
      timestamp: v.string(),
      name: v.string(),
      body: v.optional(v.string()),
      attributes: v.optional(v.string()),
      severity: v.optional(v.string()),
    })
  ),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) return [];

    const session = await ctx.db.get(args.sessionId);
    if (!session) return [];

    // Verify team membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", session.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) return [];

    const limit = args.limit ?? 500;
    return await ctx.db
      .query("otelEvents")
      .withIndex("by_session", (q) => q.eq("sessionId", args.sessionId))
      .take(limit);
  },
});
