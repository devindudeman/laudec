const BASE = '';

async function get(path) {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

export function fetchSessions() {
  return get('/api/sessions');
}

export function fetchSession(id) {
  return get(`/api/sessions/${id}`);
}

export function fetchEvents(id) {
  return get(`/api/sessions/${id}/events`);
}

export function fetchCalls(id) {
  return get(`/api/sessions/${id}/calls`);
}

export function fetchTools(id) {
  return get(`/api/sessions/${id}/tools`);
}

export function fetchConfig() {
  return get('/api/config');
}

// Formatting helpers

export function fmtDuration(secs) {
  if (secs == null) return '-';
  const s = Number(secs);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}h ${m}m`;
}

export function fmtTokens(n) {
  if (n == null) return '-';
  const v = Number(n);
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return String(v);
}

export function fmtCost(usd) {
  if (usd == null) return '-';
  return `$${Number(usd).toFixed(4)}`;
}

export function fmtTime(ts) {
  if (!ts) return '-';
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString('en-US', { hour12: false });
  } catch {
    return ts;
  }
}

export function fmtDatetime(ts) {
  if (!ts) return '-';
  try {
    const d = new Date(ts);
    return d.toLocaleString('en-US', {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
      hour12: false,
    });
  } catch {
    return ts;
  }
}

export function fmtMs(ms) {
  if (ms == null) return '-';
  const v = Number(ms);
  if (v >= 1000) return `${(v / 1000).toFixed(1)}s`;
  return `${Math.round(v)}ms`;
}

export function parseAttrs(raw) {
  if (!raw) return {};
  if (typeof raw === 'object') return raw;
  try { return JSON.parse(raw); } catch { return {}; }
}

export function pct(value, max) {
  if (!max || !value) return 0;
  return Math.max(0.5, (Number(value) / Number(max)) * 100);
}
