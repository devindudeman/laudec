import { mutation, query, internalQuery } from "./_generated/server";
import { v } from "convex/values";
import { getAuthUserId } from "@convex-dev/auth/server";

// Simple hash function for API key lookup
// In production, use a proper crypto hash via an action
function simpleHash(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = (hash << 5) - hash + char;
    hash |= 0;
  }
  return hash.toString(36);
}

function generateKey(): string {
  const chars =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let key = "ldc_";
  for (let i = 0; i < 40; i++) {
    key += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return key;
}

export const create = mutation({
  args: { teamId: v.id("teams"), name: v.string() },
  returns: v.object({
    keyId: v.id("apiKeys"),
    key: v.string(),
  }),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) throw new Error("Unauthorized");

    // Verify membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", args.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) throw new Error("Not a team member");

    const key = generateKey();
    const keyId = await ctx.db.insert("apiKeys", {
      teamId: args.teamId,
      name: args.name,
      keyHash: simpleHash(key),
      keyPrefix: key.slice(0, 8),
      createdBy: userId,
      createdAt: Date.now(),
    });

    // Return the full key only once — it's not stored
    return { keyId, key };
  },
});

export const list = query({
  args: { teamId: v.id("teams") },
  returns: v.array(
    v.object({
      _id: v.id("apiKeys"),
      _creationTime: v.number(),
      teamId: v.id("teams"),
      name: v.string(),
      keyPrefix: v.string(),
      createdBy: v.id("users"),
      createdAt: v.number(),
      lastUsedAt: v.optional(v.number()),
      revokedAt: v.optional(v.number()),
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

    const keys = await ctx.db
      .query("apiKeys")
      .withIndex("by_team", (q) => q.eq("teamId", args.teamId))
      .collect();

    return keys.map((k) => ({
      _id: k._id,
      _creationTime: k._creationTime,
      teamId: k.teamId,
      name: k.name,
      keyPrefix: k.keyPrefix,
      createdBy: k.createdBy,
      createdAt: k.createdAt,
      lastUsedAt: k.lastUsedAt,
      revokedAt: k.revokedAt,
    }));
  },
});

export const revoke = mutation({
  args: { keyId: v.id("apiKeys") },
  returns: v.null(),
  handler: async (ctx, args) => {
    const userId = await getAuthUserId(ctx);
    if (!userId) throw new Error("Unauthorized");

    const key = await ctx.db.get(args.keyId);
    if (!key) throw new Error("Key not found");

    // Verify membership
    const membership = await ctx.db
      .query("teamMembers")
      .withIndex("by_team_and_user", (q) =>
        q.eq("teamId", key.teamId).eq("userId", userId)
      )
      .unique();
    if (!membership) throw new Error("Not a team member");

    await ctx.db.patch(args.keyId, { revokedAt: Date.now() });
    return null;
  },
});

// Internal: validate an API key and return the team ID
export const validateKey = internalQuery({
  args: { keyHash: v.string() },
  returns: v.union(v.id("teams"), v.null()),
  handler: async (ctx, args) => {
    const key = await ctx.db
      .query("apiKeys")
      .withIndex("by_key_hash", (q) => q.eq("keyHash", args.keyHash))
      .unique();

    if (!key || key.revokedAt) return null;
    return key.teamId;
  },
});
