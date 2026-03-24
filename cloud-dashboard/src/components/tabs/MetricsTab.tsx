"use client";

import { fmtTokens, fmtCost, fmtMs, pct } from "@/lib/format";

type Session = {
  costUsd?: number | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheRead?: number | null;
};

type ApiCall = {
  _id: string;
  model?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheRead?: number | null;
  latencyMs?: number | null;
};

export function MetricsTab({
  session,
  calls,
}: {
  session: Session;
  calls: ApiCall[];
}) {
  if (calls.length === 0) {
    return (
      <div className="text-center py-12 text-zinc-500">No metrics data</div>
    );
  }

  let totalInput = 0,
    totalOutput = 0,
    totalCache = 0,
    totalLatency = 0;
  for (const c of calls) {
    totalInput += Number(c.inputTokens || 0);
    totalOutput += Number(c.outputTokens || 0);
    totalCache += Number(c.cacheRead || 0);
    totalLatency += Number(c.latencyMs || 0);
  }
  const totalContext = totalInput + totalCache;
  const cacheRate = totalContext > 0 ? (totalCache / totalContext) * 100 : 0;
  const avgLatency = calls.length > 0 ? totalLatency / calls.length : 0;

  const tokenTotal = totalInput + totalOutput + totalCache || 1;

  return (
    <div>
      {/* Summary cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-px bg-zinc-700 border border-zinc-700 mb-4">
        <MetricCard label="Total Cost" value={fmtCost(session.costUsd)} />
        <MetricCard label="API Calls" value={String(calls.length)} />
        <MetricCard label="Avg Latency" value={fmtMs(avgLatency)} />
        <MetricCard label="Cache Hit Rate" value={`${cacheRate.toFixed(0)}%`} />
      </div>

      {/* Token breakdown */}
      <SectionTitle>Token Breakdown</SectionTitle>
      <div className="flex gap-4 text-[10px] mb-2">
        <span className="flex items-center gap-1">
          <span className="w-2.5 h-2.5 bg-zinc-100 inline-block" />
          Input ({fmtTokens(totalInput)})
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2.5 h-2.5 bg-blue-500 inline-block" />
          Output ({fmtTokens(totalOutput)})
        </span>
        <span className="flex items-center gap-1">
          <span className="w-2.5 h-2.5 bg-zinc-500 inline-block" />
          Cache ({fmtTokens(totalCache)})
        </span>
      </div>
      <div className="flex h-6 border border-zinc-700 mb-4 overflow-hidden">
        <div
          className="bg-zinc-100 min-w-px"
          style={{ width: `${(totalInput / tokenTotal) * 100}%` }}
        />
        <div
          className="bg-blue-500 min-w-px"
          style={{ width: `${(totalOutput / tokenTotal) * 100}%` }}
        />
        <div
          className="bg-zinc-500 min-w-px"
          style={{ width: `${(totalCache / tokenTotal) * 100}%` }}
        />
      </div>

      {/* API calls table */}
      <SectionTitle>API Calls</SectionTitle>
      <div className="overflow-x-auto">
        <table className="w-full text-[12px] border border-zinc-700 border-collapse">
          <thead>
            <tr className="bg-zinc-100 text-zinc-900">
              <th className="px-2.5 py-1.5 text-left font-semibold uppercase text-[10px] tracking-wider">
                #
              </th>
              <th className="px-2.5 py-1.5 text-left font-semibold uppercase text-[10px] tracking-wider">
                Model
              </th>
              <th className="px-2.5 py-1.5 text-right font-semibold uppercase text-[10px] tracking-wider">
                Input
              </th>
              <th className="px-2.5 py-1.5 text-right font-semibold uppercase text-[10px] tracking-wider">
                Output
              </th>
              <th className="px-2.5 py-1.5 text-right font-semibold uppercase text-[10px] tracking-wider">
                Cache
              </th>
              <th className="px-2.5 py-1.5 text-right font-semibold uppercase text-[10px] tracking-wider">
                Latency
              </th>
            </tr>
          </thead>
          <tbody>
            {calls.map((c, i) => (
              <tr key={c._id} className="border-b border-zinc-800">
                <td className="px-2.5 py-1.5">{i + 1}</td>
                <td className="px-2.5 py-1.5">{c.model || "—"}</td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(c.inputTokens)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(c.outputTokens)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(c.cacheRead)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtMs(c.latencyMs)}
                </td>
              </tr>
            ))}
            {calls.length > 1 && (
              <tr className="border-t-2 border-zinc-600 font-bold">
                <td className="px-2.5 py-1.5"></td>
                <td className="px-2.5 py-1.5">Total</td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(totalInput)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(totalOutput)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtTokens(totalCache)}
                </td>
                <td className="px-2.5 py-1.5 text-right tabular-nums">
                  {fmtMs(totalLatency)}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-zinc-900 px-4 py-3">
      <div className="text-[10px] uppercase tracking-wider text-zinc-500 mb-1">
        {label}
      </div>
      <div className="text-xl font-bold">{value}</div>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[11px] font-bold uppercase tracking-wider border-b border-zinc-800 pb-1 mb-2 mt-4">
      {children}
    </div>
  );
}
