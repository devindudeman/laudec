"use client";

import { useQuery } from "convex/react";
import { api } from "../../convex/_generated/api";
import { Id } from "../../convex/_generated/dataModel";
import { useState } from "react";
import Link from "next/link";
import { fmtDuration, fmtTokens, fmtCost, fmtModel } from "@/lib/format";
import { ProxyTab } from "./tabs/ProxyTab";
import { EventsTab } from "./tabs/EventsTab";
import { MetricsTab } from "./tabs/MetricsTab";

type Tab = "proxy" | "events" | "metrics";

export function SessionDetailView({ id }: { id: Id<"sessions"> }) {
  const session = useQuery(api.sessions.get, { sessionId: id });
  const calls = useQuery(api.sessions.getCalls, { sessionId: id });
  const events = useQuery(api.sessions.getEvents, { sessionId: id });
  const [tab, setTab] = useState<Tab>("proxy");

  if (session === undefined) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-zinc-500">Loading session...</div>
      </div>
    );
  }

  if (session === null) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-zinc-500">Session not found</div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header */}
      <header className="border-b border-zinc-800 bg-zinc-900/50 px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <Link
            href="/"
            className="text-zinc-400 hover:text-zinc-200 text-sm"
          >
            ← SESSIONS
          </Link>
          <h1 className="text-lg font-bold tracking-tight">laudec cloud</h1>
        </div>
      </header>

      <main className="flex-1 p-6 max-w-[1400px] mx-auto w-full">
        {/* Stats bar */}
        <div className="grid grid-cols-2 md:grid-cols-7 gap-px bg-zinc-700 border border-zinc-700 mb-4">
          <Stat label="Duration" value={fmtDuration(session.durationSecs)} />
          <Stat label="Model" value={fmtModel(session.model)} />
          <Stat label="API Calls" value={String(session.apiCallCount ?? "—")} />
          <Stat label="Tokens In" value={fmtTokens(session.inputTokens)} />
          <Stat label="Tokens Out" value={fmtTokens(session.outputTokens)} />
          <Stat label="Cost" value={fmtCost(session.costUsd)} />
          <Stat
            label="Files Changed"
            value={String(session.filesChanged ?? "—")}
          />
        </div>

        {/* Tabs */}
        <div className="flex border border-zinc-700 w-fit mb-4">
          {(["proxy", "events", "metrics"] as Tab[]).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`px-4 py-1.5 text-xs font-semibold uppercase tracking-wider border-r border-zinc-700 last:border-r-0 transition-colors ${
                tab === t
                  ? "bg-zinc-100 text-zinc-900"
                  : "bg-zinc-900 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              {t}
            </button>
          ))}
        </div>

        {/* Tab content */}
        {tab === "proxy" && <ProxyTab calls={calls ?? []} />}
        {tab === "events" && <EventsTab events={events ?? []} />}
        {tab === "metrics" && (
          <MetricsTab session={session} calls={calls ?? []} />
        )}
      </main>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-zinc-900 px-4 py-3">
      <div className="text-[10px] uppercase tracking-wider text-zinc-500 mb-1">
        {label}
      </div>
      <div className="text-lg font-bold">{value}</div>
    </div>
  );
}
