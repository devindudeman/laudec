// Formatting helpers matching laudec's api.js

export function fmtDuration(secs: number | null | undefined): string {
  if (secs == null) return "—";
  const s = Number(secs);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}h ${m}m`;
}

export function fmtTokens(n: number | string | null | undefined): string {
  if (n == null) return "—";
  const v = Number(n);
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return String(v);
}

export function fmtCost(usd: number | string | null | undefined): string {
  if (usd == null) return "—";
  return `$${Number(usd).toFixed(4)}`;
}

export function fmtTime(ts: string | null | undefined): string {
  if (!ts) return "—";
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString("en-US", { hour12: false });
  } catch {
    return ts;
  }
}

export function fmtDatetime(ts: string | null | undefined): string {
  if (!ts) return "—";
  try {
    const d = new Date(ts);
    return d.toLocaleString("en-US", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  } catch {
    return ts;
  }
}

export function fmtMs(ms: number | string | null | undefined): string {
  if (ms == null) return "—";
  const v = Number(ms);
  if (v >= 1000) return `${(v / 1000).toFixed(1)}s`;
  return `${Math.round(v)}ms`;
}

export function fmtModel(model: string | null | undefined): string {
  if (!model) return "—";
  return model.replace(/^claude-/, "");
}

export function parseAttrs(raw: string | null | undefined): Record<string, any> {
  if (!raw) return {};
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

export function pct(value: number, max: number): number {
  if (!max || !value) return 0;
  return Math.max(0.5, (Number(value) / Number(max)) * 100);
}

export function costClass(cost: number | null | undefined): string {
  if (cost == null || cost < 0.5) return "text-emerald-400";
  if (cost < 2.0) return "text-amber-400";
  return "text-red-400 font-bold";
}
