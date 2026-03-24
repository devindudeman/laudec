import { httpRouter } from "convex/server";
import { auth } from "./auth";
import { pushSession, pushCalls, pushEvents } from "./ingest";

const http = httpRouter();

// Auth routes (OAuth callbacks etc.)
auth.addHttpRoutes(http);

// ── Ingest API (called by laudec CLI) ───────────────────────────────
http.route({
  path: "/api/ingest/session",
  method: "POST",
  handler: pushSession,
});

http.route({
  path: "/api/ingest/calls",
  method: "POST",
  handler: pushCalls,
});

http.route({
  path: "/api/ingest/events",
  method: "POST",
  handler: pushEvents,
});

export default http;
