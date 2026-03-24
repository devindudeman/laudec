// End-to-end test: simulate laudec pushing a session to the cloud dashboard
// Usage: node test-ingest.mjs <API_KEY>

const ENDPOINT = "https://acoustic-basilisk-685.convex.site";
const API_KEY = process.argv[2];

if (!API_KEY) {
  console.error("Usage: node test-ingest.mjs <API_KEY>");
  process.exit(1);
}

async function post(path, body) {
  const resp = await fetch(`${ENDPOINT}${path}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${API_KEY}`,
    },
    body: JSON.stringify(body),
  });
  const text = await resp.text();
  console.log(`${path}: ${resp.status} ${text}`);
  if (!resp.ok) throw new Error(`${resp.status}: ${text}`);
  return JSON.parse(text);
}

async function main() {
  const runId = `test-${Date.now()}`;
  console.log(`\n=== Testing cloud push (runId: ${runId}) ===\n`);

  // 1. Push active session
  console.log("1. Pushing active session...");
  const sessionResult = await post("/api/ingest/session", {
    runId,
    project: "laudec-test",
    projectPath: "/home/user/laudec",
    startedAt: new Date().toISOString(),
    status: "active",
  });
  const sessionId = sessionResult.sessionId;
  console.log(`   Session created: ${sessionId}\n`);

  // 2. Push some API calls
  console.log("2. Pushing API calls...");
  await post("/api/ingest/calls", {
    sessionId,
    calls: [
      {
        callId: `${runId}-call-1`,
        timestamp: new Date().toISOString(),
        method: "POST",
        path: "/v1/messages",
        statusCode: 200,
        latencyMs: 1250,
        model: "claude-sonnet-4-20250514",
        inputTokens: 15230,
        outputTokens: 842,
        cacheRead: 12000,
        cacheWrite: 3230,
        responseText: "I'll help you build a cloud dashboard for laudec. Let me start by...",
      },
      {
        callId: `${runId}-call-2`,
        timestamp: new Date(Date.now() + 5000).toISOString(),
        method: "POST",
        path: "/v1/messages",
        statusCode: 200,
        latencyMs: 2100,
        model: "claude-sonnet-4-20250514",
        inputTokens: 18500,
        outputTokens: 1205,
        cacheRead: 15230,
        cacheWrite: 0,
        responseText: "Here's the updated schema with the new indexes...",
      },
      {
        callId: `${runId}-call-3`,
        timestamp: new Date(Date.now() + 12000).toISOString(),
        method: "POST",
        path: "/v1/messages",
        statusCode: 200,
        latencyMs: 980,
        model: "claude-sonnet-4-20250514",
        inputTokens: 22100,
        outputTokens: 356,
        cacheRead: 18500,
        cacheWrite: 0,
        responseText: "Done! I've committed the changes.",
      },
    ],
  });
  console.log("   3 calls pushed\n");

  // 3. Push some OTEL events
  console.log("3. Pushing OTEL events...");
  await post("/api/ingest/events", {
    sessionId,
    events: [
      {
        timestamp: new Date().toISOString(),
        name: "user_prompt",
        attributes: JSON.stringify({
          prompt: "Build a cloud dashboard for laudec using Convex",
          prompt_length: "48",
          "prompt.id": "prompt-1",
        }),
      },
      {
        timestamp: new Date(Date.now() + 1000).toISOString(),
        name: "api_request",
        attributes: JSON.stringify({
          model: "claude-sonnet-4-20250514",
          input_tokens: "15230",
          output_tokens: "842",
          cache_read_tokens: "12000",
          cost_usd: "0.0523",
          duration_ms: "1250",
          "prompt.id": "prompt-1",
        }),
      },
      {
        timestamp: new Date(Date.now() + 2000).toISOString(),
        name: "tool_decision",
        attributes: JSON.stringify({
          tool_name: "Write",
          decision: "approved",
          source: "auto",
          "prompt.id": "prompt-1",
        }),
      },
      {
        timestamp: new Date(Date.now() + 2500).toISOString(),
        name: "tool_result",
        attributes: JSON.stringify({
          tool_name: "Write",
          success: "true",
          duration_ms: "45",
          "prompt.id": "prompt-1",
        }),
      },
      {
        timestamp: new Date(Date.now() + 3000).toISOString(),
        name: "tool_decision",
        attributes: JSON.stringify({
          tool_name: "Read",
          decision: "approved",
          source: "auto",
          "prompt.id": "prompt-1",
        }),
      },
      {
        timestamp: new Date(Date.now() + 3500).toISOString(),
        name: "tool_result",
        attributes: JSON.stringify({
          tool_name: "Read",
          success: "true",
          duration_ms: "12",
          "prompt.id": "prompt-1",
        }),
      },
    ],
  });
  console.log("   6 events pushed\n");

  // 4. Update session as completed
  console.log("4. Updating session as completed...");
  await post("/api/ingest/session", {
    runId,
    project: "laudec-test",
    projectPath: "/home/user/laudec",
    startedAt: new Date(Date.now() - 30000).toISOString(),
    endedAt: new Date().toISOString(),
    durationSecs: 30,
    apiCallCount: 3,
    inputTokens: 55830,
    outputTokens: 2403,
    cacheRead: 45730,
    cacheWrite: 3230,
    costUsd: 0.1847,
    model: "claude-sonnet-4-20250514",
    filesChanged: 5,
    linesAdded: 234,
    linesRemoved: 12,
    summary: 'Asked: "Build a cloud dashboard for laudec using Convex". Tools: Write (3x), Read (2x)',
    toolUses: 5,
    firstPrompt: "Build a cloud dashboard for laudec using Convex",
    errorCount: 0,
    status: "completed",
  });
  console.log("   Session marked as completed\n");

  console.log("=== SUCCESS! Check the dashboard — you should see the session ===\n");
}

main().catch((e) => {
  console.error("FAILED:", e.message);
  process.exit(1);
});
