"use client";

import { useState } from "react";
import { fmtTime, fmtMs, fmtTokens, fmtCost, parseAttrs } from "@/lib/format";

type OtelEvent = {
  _id: string;
  timestamp: string;
  name: string;
  body?: string | null;
  attributes?: string | null;
  severity?: string | null;
};

type Turn = {
  promptId: string;
  prompt: { ev: OtelEvent; attrs: Record<string, any> } | null;
  apiRequests: { ev: OtelEvent; attrs: Record<string, any> }[];
  toolOps: { ev: OtelEvent; attrs: Record<string, any>; eventName: string }[];
};

function groupIntoTurns(events: OtelEvent[]): Turn[] {
  const evAsc = [...events].reverse();
  const turnMap = new Map<string, Turn>();

  for (const ev of evAsc) {
    const attrs = parseAttrs(ev.attributes);
    const eventName =
      ev.name === "log" ? attrs["event.name"] || ev.name : ev.name;
    const promptId = attrs["prompt.id"];

    if (!promptId) continue;

    if (!turnMap.has(promptId)) {
      turnMap.set(promptId, {
        promptId,
        prompt: null,
        apiRequests: [],
        toolOps: [],
      });
    }
    const turn = turnMap.get(promptId)!;

    if (eventName === "user_prompt") {
      turn.prompt = { ev, attrs };
    } else if (eventName === "api_request") {
      turn.apiRequests.push({ ev, attrs });
    } else if (eventName === "tool_decision" || eventName === "tool_result") {
      turn.toolOps.push({ ev, attrs, eventName });
    }
  }

  return [...turnMap.values()];
}

export function EventsTab({ events }: { events: OtelEvent[] }) {
  const [expandedEvents, setExpandedEvents] = useState<
    Record<string, boolean>
  >({});
  const turns = groupIntoTurns(events);

  function toggleEvent(key: string) {
    setExpandedEvents((prev) => ({ ...prev, [key]: !prev[key] }));
  }

  if (turns.length === 0) {
    return (
      <div className="text-center py-12 text-zinc-500">No OTEL events</div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {turns.map((turn, ti) => (
        <div key={turn.promptId} className="bg-zinc-900 border border-zinc-800">
          {/* User prompt */}
          {turn.prompt && (
            <div className="px-3 py-2.5 border-b border-zinc-800">
              <div className="flex items-baseline gap-2.5 text-[11px] mb-2">
                <span className="text-zinc-600 text-[10px]">#{ti + 1}</span>
                <span className="font-bold uppercase tracking-wider text-[10px] text-blue-400 border border-blue-400 px-1.5 py-0.5">
                  YOU
                </span>
                <span className="text-zinc-500">
                  {fmtTime(turn.prompt.ev.timestamp)}
                </span>
                {turn.prompt.attrs.prompt_length && (
                  <span className="text-zinc-600 text-[10px]">
                    {turn.prompt.attrs.prompt_length} chars
                  </span>
                )}
              </div>
              <div className="text-[13px] leading-relaxed border-l-[3px] border-blue-500 pl-3 bg-blue-950/10 py-2 whitespace-pre-wrap break-words">
                {turn.prompt.attrs.prompt || "(empty)"}
              </div>
            </div>
          )}

          {/* API requests */}
          {turn.apiRequests.map((req, ri) => {
            const key = `${ti}-api-${ri}`;
            return (
              <div key={key} className="px-3 py-2 border-b border-zinc-800">
                <div
                  className="flex items-baseline gap-2.5 text-[11px] cursor-pointer hover:bg-zinc-800/50 mb-1"
                  onClick={() => toggleEvent(key)}
                >
                  <span className="font-bold uppercase tracking-wider text-[10px] text-zinc-300 border border-zinc-500 px-1.5 py-0.5">
                    API
                  </span>
                  <span className="text-zinc-500">
                    {fmtTime(req.ev.timestamp)}
                  </span>
                  <span className="font-semibold">
                    {req.attrs.model || "—"}
                  </span>
                  <span className="text-zinc-500">
                    {fmtMs(req.attrs.duration_ms)}
                  </span>
                  <span className="ml-auto text-[10px] text-zinc-600">
                    {expandedEvents[key] ? "▼" : "▶"}
                  </span>
                </div>
                <div className="flex items-center gap-1.5 flex-wrap">
                  <span className="text-[10px] px-1.5 py-0.5 bg-zinc-100 text-zinc-900 border border-zinc-100">
                    {fmtTokens(req.attrs.input_tokens)} in
                  </span>
                  <span className="text-[10px] px-1.5 py-0.5 bg-blue-600 text-white border border-blue-600">
                    {fmtTokens(req.attrs.output_tokens)} out
                  </span>
                  {Number(req.attrs.cache_read_tokens) > 0 && (
                    <span className="text-[10px] px-1.5 py-0.5 bg-zinc-600 text-zinc-200 border border-zinc-600">
                      {fmtTokens(req.attrs.cache_read_tokens)} cache
                    </span>
                  )}
                  {req.attrs.cost_usd && (
                    <span className="text-[10px] px-1.5 py-0.5 bg-zinc-900 text-zinc-100 border border-zinc-100 font-bold">
                      {fmtCost(req.attrs.cost_usd)}
                    </span>
                  )}
                </div>
                {expandedEvents[key] && (
                  <pre className="mt-2 text-[11px] text-zinc-400 overflow-x-auto max-h-[300px] overflow-y-auto whitespace-pre-wrap">
                    {JSON.stringify(req.attrs, null, 2)}
                  </pre>
                )}
              </div>
            );
          })}

          {/* Tool operations */}
          {turn.toolOps.length > 0 && (
            <div className="border-t border-zinc-800">
              {turn.toolOps.map((op, oi) => {
                const key = `${ti}-tool-${oi}`;
                const isResult = op.eventName === "tool_result";
                const success = op.attrs.success === "true";

                return (
                  <div
                    key={key}
                    className="px-3 pl-6 py-1 border-b border-zinc-800/50 text-[11px] last:border-b-0"
                  >
                    <div
                      className="flex items-baseline gap-2 cursor-pointer hover:bg-zinc-800/50"
                      onClick={() => toggleEvent(key)}
                    >
                      <span
                        className={`font-bold w-3 text-center shrink-0 ${
                          isResult
                            ? success
                              ? "text-emerald-400"
                              : "text-red-400"
                            : "text-zinc-500"
                        }`}
                      >
                        {isResult ? (success ? "+" : "!") : ">"}
                      </span>
                      <span className="font-semibold shrink-0">
                        {op.attrs.tool_name || "—"}
                      </span>
                      {isResult && (
                        <span
                          className={`font-semibold ${
                            success ? "text-emerald-400" : "text-red-400"
                          }`}
                        >
                          {success ? "OK" : "FAIL"}
                        </span>
                      )}
                      {!isResult && (
                        <span className="text-zinc-500">
                          {op.attrs.decision || "—"} ({op.attrs.source || "—"})
                        </span>
                      )}
                      {op.attrs.duration_ms && (
                        <span className="text-zinc-500">
                          {fmtMs(op.attrs.duration_ms)}
                        </span>
                      )}
                      <span className="ml-auto text-[10px] text-zinc-600">
                        {expandedEvents[key] ? "▼" : "▶"}
                      </span>
                    </div>
                    {op.attrs.error && (
                      <div className="text-red-400 text-[11px] pl-5 mt-1">
                        {op.attrs.error}
                      </div>
                    )}
                    {expandedEvents[key] && (
                      <pre className="mt-2 ml-5 text-[11px] text-zinc-400 overflow-x-auto max-h-[300px] overflow-y-auto whitespace-pre-wrap">
                        {JSON.stringify(op.attrs, null, 2)}
                      </pre>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
