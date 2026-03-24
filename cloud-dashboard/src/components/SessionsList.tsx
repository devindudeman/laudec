"use client";

import { useQuery } from "convex/react";
import { api } from "../../convex/_generated/api";
import { Id } from "../../convex/_generated/dataModel";
import Link from "next/link";
import {
  fmtDuration,
  fmtTokens,
  fmtCost,
  fmtDatetime,
  fmtModel,
  costClass,
} from "@/lib/format";

export function SessionsList({ teamId }: { teamId: Id<"teams"> }) {
  const sessions = useQuery(api.sessions.list, { teamId });

  if (sessions === undefined) {
    return <div className="text-zinc-500">Loading sessions...</div>;
  }

  if (sessions.length === 0) {
    return (
      <div className="text-center py-16">
        <h3 className="text-lg font-medium text-zinc-300 mb-2">
          No sessions yet
        </h3>
        <p className="text-zinc-500 text-sm max-w-md mx-auto">
          Configure laudec to push to this dashboard. Create an API key in
          Settings, then add the cloud endpoint to your laudec.toml.
        </p>
      </div>
    );
  }

  // Compute totals
  let totalCalls = 0,
    totalCost = 0,
    totalErrors = 0,
    totalDur = 0;
  for (const s of sessions) {
    totalCalls += s.apiCallCount ?? 0;
    totalCost += s.costUsd ?? 0;
    totalErrors += s.errorCount ?? 0;
    totalDur += s.durationSecs ?? 0;
  }

  function sessionTitle(s: (typeof sessions)[0]): string {
    const text = s.firstPrompt || s.summary || "—";
    return text.length > 80 ? text.slice(0, 80) + "..." : text;
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full border border-zinc-700 border-collapse text-[12px]">
        <thead>
          <tr className="bg-zinc-100 text-zinc-900">
            <Th>Time</Th>
            <Th>Dur</Th>
            <Th>Project</Th>
            <Th>Model</Th>
            <Th align="right">Calls</Th>
            <Th align="right">Cost</Th>
            <Th>Title</Th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr
              key={s._id}
              className="border-b border-zinc-800 hover:bg-zinc-800/50 cursor-pointer"
            >
              <td className="px-2.5 py-1.5 whitespace-nowrap">
                <Link
                  href={`/session/${s._id}`}
                  className="text-zinc-300 hover:text-white"
                >
                  {fmtDatetime(s.startedAt)}
                  {s.status === "active" && (
                    <span className="ml-1.5 text-[9px] font-bold uppercase tracking-wider bg-emerald-600 text-white px-1 py-0.5 rounded-sm animate-pulse">
                      LIVE
                    </span>
                  )}
                </Link>
              </td>
              <td className="px-2.5 py-1.5 whitespace-nowrap">
                {fmtDuration(s.durationSecs)}
              </td>
              <td className="px-2.5 py-1.5 whitespace-nowrap">
                {s.project || "—"}
              </td>
              <td className="px-2.5 py-1.5 whitespace-nowrap">
                {fmtModel(s.model)}
              </td>
              <td className="px-2.5 py-1.5 text-right whitespace-nowrap tabular-nums">
                {s.apiCallCount ?? "—"}
                {(s.errorCount ?? 0) > 0 && (
                  <span className="ml-1 text-[9px] font-bold bg-red-600 text-white px-1 py-0.5 rounded-sm">
                    {s.errorCount} err
                  </span>
                )}
              </td>
              <td
                className={`px-2.5 py-1.5 text-right whitespace-nowrap tabular-nums ${costClass(s.costUsd)}`}
              >
                {fmtCost(s.costUsd)}
              </td>
              <td className="px-2.5 py-1.5 max-w-[300px] overflow-hidden text-ellipsis whitespace-nowrap text-zinc-400">
                {sessionTitle(s)}
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="border-t-2 border-zinc-600 font-bold">
            <td className="px-2.5 py-1.5" colSpan={2}>
              {sessions.length} sessions · {fmtDuration(totalDur)}
            </td>
            <td></td>
            <td></td>
            <td className="px-2.5 py-1.5 text-right tabular-nums">
              {totalCalls}
              {totalErrors > 0 && (
                <span className="ml-1 text-[9px] font-bold bg-red-600 text-white px-1 py-0.5 rounded-sm">
                  {totalErrors} err
                </span>
              )}
            </td>
            <td className={`px-2.5 py-1.5 text-right tabular-nums ${costClass(totalCost)}`}>
              {fmtCost(totalCost)}
            </td>
            <td></td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={`px-2.5 py-1.5 font-semibold uppercase text-[10px] tracking-wider whitespace-nowrap ${
        align === "right" ? "text-right" : "text-left"
      }`}
    >
      {children}
    </th>
  );
}
