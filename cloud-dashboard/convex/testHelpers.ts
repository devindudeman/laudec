import { internalMutation } from "./_generated/server";
import { v } from "convex/values";

// Simple hash function — must match apiKeys.ts
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

// Internal-only: create an API key without auth check (for testing)
export const createTestApiKey = internalMutation({
  args: { teamId: v.id("teams"), name: v.string() },
  returns: v.object({ key: v.string(), keyId: v.id("apiKeys") }),
  handler: async (ctx, args) => {
    const team = await ctx.db.get(args.teamId);
    if (!team) throw new Error("Team not found");

    const key = generateKey();
    const keyId = await ctx.db.insert("apiKeys", {
      teamId: args.teamId,
      name: args.name,
      keyHash: simpleHash(key),
      keyPrefix: key.slice(0, 8),
      createdBy: team.ownerId,
      createdAt: Date.now(),
    });

    return { key, keyId };
  },
});
