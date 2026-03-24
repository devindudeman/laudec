"use client";

import { useQuery } from "convex/react";
import { api } from "../../convex/_generated/api";
import { Id } from "../../convex/_generated/dataModel";

function formatDuration(secs: number | undefined): string {
  if (!secs) return "—";
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

function formatTokens(n: number | undefined): string {
  if (!n) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return n.toString();
}

export function SessionsList({ teamId }: { teamId: Id<"teams"> }) {
  const sessions = useQuery(api.sessions.list, { teamId });

  if (sessions === undefined) {
    return <div className="text-gray-400">Loading sessions...</div>;
  }

  if (sessions.length === 0) {
    return (
      <div className="text-center py-16">
        <h3 className="text-lg font-medium text-gray-300 mb-2">
          No sessions yet
        </h3>
        <p className="text-gray-500 text-sm max-w-md mx-auto">
          Configure laudec to push to this dashboard. Create an API key in
          Settings, then add the cloud endpoint to your laudec.toml.
        </p>
      </div>
    );
  }

  return (
    <div>
      <h2 className="text-lg font-semibold mb-4">Sessions</h2>
      <div className="space-y-2">
        {sessions.map((s) => (
          <div
            key={s._id}
            className="bg-gray-900 border border-gray-800 rounded-lg p-4 hover:border-gray-700 transition-colors cursor-pointer"
          >
            <div className="flex items-center justify-between mb-2">
              <div className="flex items-center gap-3">
                <span className="font-medium">{s.project}</span>
                <span
                  className={`text-xs px-2 py-0.5 rounded-full ${
                    s.status === "active"
                      ? "bg-green-900/50 text-green-400 border border-green-800"
                      : "bg-gray-800 text-gray-400"
                  }`}
                >
                  {s.status}
                </span>
                {s.model && (
                  <span className="text-xs text-gray-500">{s.model}</span>
                )}
              </div>
              <span className="text-xs text-gray-500">
                {s.startedAt.slice(0, 16).replace("T", " ")}
              </span>
            </div>

            {s.summary && (
              <p className="text-sm text-gray-400 mb-2 truncate">{s.summary}</p>
            )}

            <div className="flex gap-4 text-xs text-gray-500">
              <span>{formatDuration(s.durationSecs)}</span>
              <span>{s.apiCallCount ?? 0} calls</span>
              <span>
                {formatTokens(s.inputTokens)} in / {formatTokens(s.outputTokens)}{" "}
                out
              </span>
              {s.costUsd != null && s.costUsd > 0 && (
                <span>${s.costUsd.toFixed(4)}</span>
              )}
              {s.filesChanged != null && s.filesChanged > 0 && (
                <span>
                  +{s.linesAdded ?? 0}/-{s.linesRemoved ?? 0} in{" "}
                  {s.filesChanged}f
                </span>
              )}
              {s.errorCount != null && s.errorCount > 0 && (
                <span className="text-red-400">
                  {s.errorCount} error{s.errorCount > 1 ? "s" : ""}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
