"use client";

import { useState } from "react";
import { fmtTime, fmtMs, fmtTokens } from "@/lib/format";

type ApiCall = {
  _id: string;
  callId: string;
  timestamp: string;
  method: string;
  path: string;
  statusCode?: number | null;
  latencyMs?: number | null;
  model?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  responseText?: string | null;
  requestBody?: string | null;
  responseBody?: string | null;
  requestHeaders?: string | null;
  responseHeaders?: string | null;
};

function classifyCall(call: ApiCall) {
  let type = "UNKNOWN";
  let detail: string | null = null;
  let tools: [string, number][] = [];

  let body: any = {};
  try {
    body = JSON.parse(call.requestBody || "{}");
  } catch {}

  if (body.max_tokens === 1) return { type: "QUOTA", detail: null, tools: [] };
  if ((call.path || "").includes("count_tokens"))
    return { type: "TOKEN COUNT", detail: null, tools: [] };
  if (body.thinking) {
    type = "MAIN";
  } else if (body.system && body.tools) {
    type = "SUBAGENT";
    const sys = JSON.stringify(body.system);
    if (sys.includes("file search specialist") || sys.includes("READ-ONLY MODE"))
      detail = "EXPLORE";
    else if (sys.includes("web search tool use")) detail = "WEB SEARCH";
    else if (sys.includes("Claude Code Guide")) detail = "CC GUIDE";
  }

  const msgs = body.messages || [];
  const toolCounts: Record<string, number> = {};
  for (const m of msgs) {
    if (m.role === "assistant" && Array.isArray(m.content)) {
      for (const block of m.content) {
        if (block.type === "tool_use" && block.name) {
          toolCounts[block.name] = (toolCounts[block.name] || 0) + 1;
        }
      }
    }
  }
  tools = Object.entries(toolCounts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 4);

  return { type, detail, tools };
}

const TYPE_COLORS: Record<string, string> = {
  MAIN: "bg-zinc-100 text-zinc-900",
  SUBAGENT: "bg-purple-600 text-white",
  QUOTA: "bg-zinc-600 text-zinc-200",
  "TOKEN COUNT": "bg-zinc-600 text-zinc-200",
  UNKNOWN: "bg-zinc-700 text-zinc-300",
};

export function ProxyTab({ calls }: { calls: ApiCall[] }) {
  const [expandedCalls, setExpandedCalls] = useState<Record<number, boolean>>({});
  const [collapsedConv, setCollapsedConv] = useState<Record<number, boolean>>({});

  if (calls.length === 0) {
    return <div className="text-center py-12 text-zinc-500">No proxy data</div>;
  }

  // Classify + label calls
  let turnNum = 0;
  const labels = calls.map((c) => {
    const label = classifyCall(c);
    if (label.type === "MAIN") {
      let body: any = {};
      try { body = JSON.parse(c.requestBody || "{}"); } catch {}
      const msgs = body.messages || [];
      const lastUser = [...msgs].reverse().find((m: any) => m.role === "user");
      const hasToolResult =
        lastUser &&
        Array.isArray(lastUser.content) &&
        lastUser.content.some((b: any) => b.type === "tool_result");
      if (hasToolResult) {
        label.detail = "TOOL LOOP";
      } else {
        turnNum++;
        label.detail = `TURN ${turnNum}`;
      }
    }
    return label;
  });

  function toggleCall(i: number) {
    setExpandedCalls((prev) => ({ ...prev, [i]: !prev[i] }));
  }

  function toggleConv(i: number, e: React.MouseEvent) {
    e.stopPropagation();
    setCollapsedConv((prev) => ({ ...prev, [i]: !prev[i] }));
  }

  // Extract user query and model response from request body
  function extractConversation(c: ApiCall) {
    let userText: string | null = null;
    let modelResponse = c.responseText || null;
    try {
      const body = JSON.parse(c.requestBody || "{}");
      const msgs = body.messages || [];
      for (let i = msgs.length - 1; i >= 0; i--) {
        if (msgs[i].role === "user") {
          const content = msgs[i].content;
          if (typeof content === "string") {
            userText = content;
          } else if (Array.isArray(content)) {
            const texts = content
              .filter((b: any) => b.type === "text")
              .map((b: any) => b.text);
            userText = texts.join("\n");
          }
          break;
        }
      }
    } catch {}
    // Strip system blocks from user text
    if (userText) {
      userText = userText
        .replace(/<(system-reminder|available-deferred-tools|tool-use-rules|functions)>[\s\S]*?<\/\1>/g, "")
        .replace(/\n{3,}/g, "\n\n")
        .trim() || null;
    }
    return { userText, modelResponse };
  }

  return (
    <div className="flex flex-col gap-2">
      {calls.map((c, i) => {
        const label = labels[i];
        const conv = extractConversation(c);
        const hasConv = !!(conv.userText || conv.modelResponse);
        const isError = (c.statusCode ?? 0) >= 400;
        const borderColor =
          label.type === "MAIN"
            ? "border-l-zinc-100"
            : label.type === "SUBAGENT"
            ? "border-l-purple-500"
            : "border-l-zinc-600";

        return (
          <div
            key={c._id}
            className={`bg-zinc-900 border border-zinc-800 border-l-[3px] ${borderColor} ${
              isError ? "border-red-500 bg-red-950/20" : ""
            }`}
          >
            {/* Summary row */}
            <div
              className="flex items-baseline gap-2.5 px-3 py-2.5 text-[11px] cursor-pointer hover:bg-zinc-800/50 flex-wrap"
              onClick={() => toggleCall(i)}
            >
              <span className="text-zinc-500 shrink-0">
                {fmtTime(c.timestamp)}
              </span>
              <span
                className={`text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded-sm shrink-0 ${
                  TYPE_COLORS[label.type] || TYPE_COLORS.UNKNOWN
                }`}
              >
                {label.detail || label.type}
              </span>
              {isError && (
                <span className="text-red-400 font-bold shrink-0">
                  {c.statusCode}
                </span>
              )}
              {hasConv && (
                <button
                  className="text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.5 border border-zinc-600 text-zinc-500 hover:text-zinc-200 hover:border-zinc-400 shrink-0"
                  onClick={(e) => toggleConv(i, e)}
                >
                  CONV {collapsedConv[i] ? "▶" : "▼"}
                </button>
              )}
              <span className="text-zinc-500 shrink-0">
                {c.model || "—"}
              </span>
              <span className="text-zinc-500 shrink-0">
                {fmtMs(c.latencyMs)}
              </span>
              <span className="ml-auto text-zinc-500 shrink-0">
                {fmtTokens(c.inputTokens)} in / {fmtTokens(c.outputTokens)} out
                {c.cacheRead ? (
                  <span className="ml-2 text-[10px]">
                    cache: {fmtTokens(c.cacheRead)}
                  </span>
                ) : null}
              </span>
              {label.tools.length > 0 && (
                <span className="text-[10px] text-zinc-600 shrink-0">
                  {label.tools
                    .map(([name, count]) =>
                      count > 1 ? `${name} ×${count}` : name
                    )
                    .join(" · ")}
                </span>
              )}
              <span className="text-[10px] text-zinc-600 shrink-0">
                RAW {expandedCalls[i] ? "▼" : "▶"}
              </span>
            </div>

            {/* Conversation */}
            {hasConv && !collapsedConv[i] && (
              <div className="border-t border-zinc-800">
                {conv.userText && (
                  <div className="px-3 py-2.5">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-blue-400 border border-blue-400 px-1.5 py-0.5 inline-block mb-2">
                      YOU
                    </span>
                    <div className="text-xs leading-relaxed border-l-[3px] border-blue-500 pl-3 bg-blue-950/10 py-2 whitespace-pre-wrap break-words max-h-[200px] overflow-y-auto">
                      {conv.userText}
                    </div>
                  </div>
                )}
                {conv.modelResponse && (
                  <div className="px-3 py-2.5 border-t border-zinc-800">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-zinc-300 border border-zinc-500 px-1.5 py-0.5 inline-block mb-2">
                      MODEL
                    </span>
                    <div className="text-xs leading-relaxed border-l-[3px] border-zinc-500 pl-3 bg-zinc-800/30 py-2 whitespace-pre-wrap break-words max-h-[200px] overflow-y-auto">
                      {conv.modelResponse}
                    </div>
                  </div>
                )}
              </div>
            )}

            {/* Raw details */}
            {expandedCalls[i] && (
              <div className="border-t-2 border-zinc-700 bg-zinc-950/50">
                <div className="text-[9px] font-bold uppercase tracking-wider text-zinc-600 px-3 py-1">
                  Raw
                </div>
                {c.requestHeaders && (
                  <CollapsibleSection title="Request Headers" content={c.requestHeaders} />
                )}
                {c.responseHeaders && (
                  <CollapsibleSection title="Response Headers" content={c.responseHeaders} />
                )}
                {c.requestBody && (
                  <CollapsibleSection title="Request Body" content={c.requestBody} />
                )}
                {c.responseBody && (
                  <CollapsibleSection title="Response Body" content={c.responseBody} />
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function CollapsibleSection({
  title,
  content,
}: {
  title: string;
  content: string;
}) {
  const [open, setOpen] = useState(false);

  let formatted = content;
  try {
    formatted = JSON.stringify(JSON.parse(content), null, 2);
  } catch {}

  return (
    <div className="border-b border-zinc-800 last:border-b-0">
      <button
        onClick={() => setOpen(!open)}
        className="w-full text-left px-3 py-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-500 hover:text-zinc-300"
      >
        {title} {open ? "▼" : "▶"}
      </button>
      {open && (
        <pre className="px-3 pb-3 text-[11px] overflow-x-auto max-h-[400px] overflow-y-auto whitespace-pre-wrap break-all text-zinc-400">
          {formatted}
        </pre>
      )}
    </div>
  );
}
