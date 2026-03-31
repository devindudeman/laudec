<script>
  import { onMount } from 'svelte';
  import { marked } from 'marked';
  import {
    fetchSession, fetchEvents, fetchCalls, fetchTools, fetchInsights,
    fmtDuration, fmtTokens, fmtCost, fmtTime, fmtMs, parseAttrs, pct,
  } from '../lib/api.js';

  marked.setOptions({ breaks: true, gfm: true });

  function renderMarkdown(text) {
    if (!text) return '';
    return marked.parse(text);
  }

  let { id } = $props();

  let detail = $state(null);
  let calls = $state([]);
  let events = $state([]);
  let tools = $state([]);
  let tab = $state('proxy');
  let loading = $state(true);
  let error = $state(null);
  let callViewMode = $state({});  // null | 'conv' | 'inspect' | 'raw'
  let expandedEvents = $state({});
  let insights = $state(null);

  async function load() {
    try {
      const [d, c] = await Promise.all([
        fetchSession(id),
        fetchCalls(id),
      ]);
      detail = d;
      calls = c;
      error = null;
      // Refresh tab-specific data if already loaded
      if (events.length > 0) {
        events = await fetchEvents(id);
      }
      if (tools.length > 0) {
        tools = await fetchTools(id);
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  });

  async function loadEvents() {
    if (events.length === 0) {
      try { events = await fetchEvents(id); } catch (e) { console.error('Failed to load events:', e); }
    }
  }

  async function loadMetrics() {
    if (events.length === 0 || tools.length === 0) {
      try {
        const [ev, t] = await Promise.all([fetchEvents(id), fetchTools(id)]);
        events = ev;
        tools = t;
      } catch (e) { console.error('Failed to load metrics:', e); }
    }
  }

  async function loadInsights() {
    if (!insights) {
      try { insights = await fetchInsights(id); } catch (e) { console.error('Failed to load insights:', e); }
    }
  }

  function switchTab(t) {
    tab = t;
    if (t === 'events') loadEvents();
    if (t === 'metrics') loadMetrics();
    if (t === 'insights') loadInsights();
  }

  function setCallView(i, mode, e) {
    if (e) e.stopPropagation();
    // If clicking the active mode (or the default), collapse. Otherwise switch.
    const current = callViewMode[i] ?? null;
    callViewMode = { ...callViewMode, [i]: current === mode ? 'collapsed' : mode };
  }

  /** Resolve effective view mode — CONV is default for cards with conversation data */
  function effectiveView(i, hasConv) {
    const mode = callViewMode[i];
    if (mode === 'collapsed') return null;
    if (mode) return mode;
    // Default: show CONV if the card has conversation content
    return hasConv ? 'conv' : null;
  }

  function toggleEvent(key) {
    expandedEvents = { ...expandedEvents, [key]: !expandedEvents[key] };
  }

  function eventKeyFields(eventName, attrs) {
    switch (eventName) {
      case 'user_prompt':
        return [
          { label: 'Prompt', value: attrs.prompt, cls: 'prompt-val' },
          attrs.prompt_length && { label: 'Length', value: `${attrs.prompt_length} chars` },
        ].filter(Boolean);
      case 'api_request':
        return [
          { label: 'Model', value: attrs.model },
          { label: 'Input', value: fmtTokens(attrs.input_tokens) },
          { label: 'Output', value: fmtTokens(attrs.output_tokens) },
          Number(attrs.cache_read_tokens) > 0 && { label: 'Cache Read', value: fmtTokens(attrs.cache_read_tokens) },
          Number(attrs.cache_creation_tokens) > 0 && { label: 'Cache Write', value: fmtTokens(attrs.cache_creation_tokens) },
          attrs.cost_usd && { label: 'Cost', value: `$${Number(attrs.cost_usd).toFixed(4)}` },
          attrs.duration_ms && { label: 'Duration', value: fmtMs(attrs.duration_ms) },
          attrs.speed && { label: 'Speed', value: attrs.speed },
        ].filter(Boolean);
      case 'tool_decision':
        return [
          { label: 'Tool', value: attrs.tool_name },
          { label: 'Decision', value: attrs.decision },
          { label: 'Source', value: attrs.source },
        ].filter(f => f.value != null);
      case 'tool_result':
        return [
          { label: 'Tool', value: attrs.tool_name },
          { label: 'Success', value: attrs.success },
          attrs.duration_ms && { label: 'Duration', value: fmtMs(attrs.duration_ms) },
          attrs.tool_result_size_bytes && { label: 'Result Size', value: Number(attrs.tool_result_size_bytes) >= 1000 ? `${(Number(attrs.tool_result_size_bytes)/1000).toFixed(1)}KB` : `${attrs.tool_result_size_bytes}B` },
          attrs.error && { label: 'Error', value: attrs.error },
        ].filter(Boolean);
      default:
        return Object.entries(attrs).slice(0, 8).map(([k, v]) => ({ label: k, value: String(v) }));
    }
  }

  function rawAttrsJson(attrs) {
    try { return JSON.stringify(attrs, null, 2); } catch { return '{}'; }
  }

  const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

  /** Syntax-highlight a JSON string into HTML spans */
  function highlightJson(str) {
    if (!str) return '';
    return esc(str)
      .replace(/"([^"\\]*(\\.[^"\\]*)*)"(\s*:)?/g, (match, key, _esc, colon) => {
        if (colon) return `<span class="json-key">"${key}"</span>:`;
        return `<span class="json-str">"${key}"</span>`;
      })
      .replace(/\b(-?\d+\.?\d*([eE][+-]?\d+)?)\b/g, '<span class="json-num">$1</span>')
      .replace(/\b(true|false)\b/g, '<span class="json-bool">$1</span>')
      .replace(/\bnull\b/g, '<span class="json-null">null</span>');
  }

  /** Find the best "identity" key from an object — first short string field */
  const IDENTITY_KEYS = ['name', 'tool_name', 'id', 'type', 'role', 'key'];
  function identityKey(obj) {
    if (!obj || typeof obj !== 'object') return null;
    // Prefer well-known keys
    for (const k of IDENTITY_KEYS) {
      if (typeof obj[k] === 'string' && obj[k].length < 60) return k;
    }
    // Fall back to first short string value
    for (const [k, v] of Object.entries(obj)) {
      if (typeof v === 'string' && v.length < 60) return k;
    }
    return null;
  }

  /** Preview for a collapsed object — show identity value or key names */
  function objectPreview(obj) {
    const idKey = identityKey(obj);
    if (idKey) return `"${obj[idKey]}"`;
    const keys = Object.keys(obj);
    return keys.slice(0, 4).join(', ') + (keys.length > 4 ? ', ...' : '');
  }

  /** Preview for a collapsed array — show identity values of items */
  function arrayPreview(arr) {
    if (arr.length === 0) return '';
    // If items are objects with an identity field, list the values
    if (typeof arr[0] === 'object' && arr[0] !== null) {
      const idKey = identityKey(arr[0]);
      if (idKey) {
        const names = arr.filter(v => v && typeof v[idKey] === 'string').map(v => v[idKey]);
        const shown = names.slice(0, 6);
        const rest = names.length > 6 ? ` +${names.length - 6} more` : '';
        return shown.join(', ') + rest;
      }
    }
    return `${arr.length} item${arr.length !== 1 ? 's' : ''}`;
  }

  /** Collapsible JSON tree — large nodes start collapsed */
  function jsonTreeHtml(val, indent = 0, key = null) {
    const pad = '  '.repeat(indent);
    const keyPrefix = key !== null ? `<span class="json-key">"${esc(key)}"</span>: ` : '';

    if (val === null) return `${pad}${keyPrefix}<span class="json-null">null</span>`;
    if (typeof val === 'boolean') return `${pad}${keyPrefix}<span class="json-bool">${val}</span>`;
    if (typeof val === 'number') return `${pad}${keyPrefix}<span class="json-num">${val}</span>`;
    if (typeof val === 'string') {
      const truncated = val.length > 200 ? esc(val.slice(0, 200)) + '...' : esc(val);
      return `${pad}${keyPrefix}<span class="json-str">"${truncated}"</span>`;
    }

    const isArray = Array.isArray(val);
    const open = isArray ? '[' : '{';
    const close = isArray ? ']' : '}';
    const count = isArray ? val.length : Object.keys(val).length;

    if (count === 0) return `${pad}${keyPrefix}${open}${close}`;

    // Top-level (indent 0) always expanded; children collapse when large
    const rawSize = JSON.stringify(val).length;
    const collapsed = indent > 0 && rawSize > 500;

    const childLines = isArray
      ? val.map((v, i) => jsonTreeHtml(v, indent + 1) + (i < count - 1 ? ',' : ''))
      : Object.entries(val).map(([k, v], i) => jsonTreeHtml(v, indent + 1, k) + (i < count - 1 ? ',' : ''));

    const preview = isArray ? arrayPreview(val) : objectPreview(val);

    if (collapsed) {
      return `${pad}${keyPrefix}<details class="json-fold"><summary>${open} <span class="json-preview">${esc(preview)}</span> ${close}</summary>\n${childLines.join('\n')}\n${pad}${close}</details>`;
    }
    return `${pad}${keyPrefix}${open}\n${childLines.join('\n')}\n${pad}${close}`;
  }

  /** Pretty-print JSON string as collapsible tree */
  function prettyJsonHtml(str) {
    if (!str) return '';
    try {
      return jsonTreeHtml(JSON.parse(str));
    } catch {
      return esc(str);
    }
  }

  /** Format SSE stream or JSON response body with highlighting */
  function formatResponseBodyHtml(str) {
    if (!str) return '';
    // Plain JSON
    try {
      const parsed = JSON.parse(str);
      return jsonTreeHtml(parsed);
    } catch { /* not plain JSON, try SSE */ }
    // SSE: highlight each event block
    const escaped = esc(str);
    return escaped.replace(/^(event: .+)$/gm, '<span class="sse-event">$1</span>')
      .replace(/^data: (.+)$/gm, (_match, json) => {
        const raw = json.replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>');
        try {
          const parsed = JSON.parse(raw);
          return '<span class="sse-data">data: </span>' + jsonTreeHtml(parsed, 1);
        } catch { return 'data: ' + json; }
      });
  }

  /** Extract inner content from a system XML block and render as markdown */
  function renderSystemBlock(block) {
    // Pull out the tag name and inner content
    const m = block.match(/^<(\w[\w-]*)>([\s\S]*)<\/\1>$/);
    if (!m) return { tag: 'unknown', html: marked.parse(block) };
    return { tag: m[1], html: marked.parse(m[2].trim()) };
  }

  const SYSTEM_TAG_RE = /<(system-reminder|available-deferred-tools|tool-use-rules|functions)>[\s\S]*?<\/\1>/g;

  /** Parse request body into structured sections for the INSPECT view */
  function parseRequestBody(str) {
    if (!str) return null;
    let raw;
    try { raw = JSON.parse(str); } catch { return null; }

    const model = raw.model || null;
    const max_tokens = raw.max_tokens || null;
    const thinking = raw.thinking || null;
    const effort = raw.output_config?.effort || null;
    const stream = raw.stream ?? null;

    // System blocks
    const system = (raw.system || []).map((s, i) => ({
      text: s.text || '',
      cache_control: s.cache_control || null,
      preview: (s.text || '').slice(0, 100).replace(/\n/g, ' '),
    }));

    // Messages
    const messages = (raw.messages || []).map((m, i) => {
      const isString = typeof m.content === 'string';
      const blocks = isString ? [{ type: 'text', text: m.content }] : (m.content || []);
      const blockCounts = { text: 0, tool_use: 0, tool_result: 0, thinking: 0 };
      const toolUseNames = [];
      let textPreview = '';

      for (const b of blocks) {
        const t = b.type || 'text';
        if (t in blockCounts) blockCounts[t]++;
        if (t === 'tool_use' && b.name) toolUseNames.push(b.name);
        if (t === 'text' && !textPreview) {
          const clean = (b.text || '').replace(/<[^>]+>[\s\S]*?<\/[^>]+>/g, '').trim();
          textPreview = clean.slice(0, 80);
        }
      }

      return {
        role: m.role,
        index: i,
        blocks,
        blockCounts,
        toolUseNames,
        textPreview,
        estimatedTokens: Math.ceil(JSON.stringify(m).length / 4),
      };
    });

    // Tools grouped by source
    const tools = (raw.tools || []).map(t => {
      const name = t.name || '';
      const mcp = name.match(/^mcp__([^_]+)__(.+)$/);
      return {
        name,
        displayName: mcp ? mcp[2] : name,
        source: mcp ? 'mcp' : (t.type ? 'builtin' : 'native'),
        server: mcp ? mcp[1] : null,
        description: t.description ? (t.description.length > 120 ? t.description.slice(0, 120) + '...' : t.description) : null,
        input_schema: t.input_schema || null,
        type: t.type || null,
      };
    });

    // Group tools by source
    const toolGroups = new Map();
    for (const t of tools) {
      const key = t.server || t.source;
      if (!toolGroups.has(key)) toolGroups.set(key, []);
      toolGroups.get(key).push(t);
    }

    return { raw, model, max_tokens, thinking, effort, stream, system, messages, tools, toolGroups };
  }

  /** Parse tool_use JSON from response_tool_use field */
  function parseToolCalls(toolUseJson) {
    if (!toolUseJson) return [];
    try { return JSON.parse(toolUseJson); } catch { return []; }
  }

  /** Extract tool_result blocks from the last user message in request body */
  function extractToolResults(requestBody) {
    if (!requestBody) return [];
    try {
      const body = JSON.parse(requestBody);
      const msgs = body.messages || [];
      // Find the last user message
      for (let i = msgs.length - 1; i >= 0; i--) {
        if (msgs[i].role === 'user' && Array.isArray(msgs[i].content)) {
          return msgs[i].content
            .filter(b => b.type === 'tool_result')
            .map(b => ({
              tool_use_id: b.tool_use_id,
              content: typeof b.content === 'string'
                ? b.content
                : Array.isArray(b.content)
                  ? b.content.filter(c => c.type === 'text').map(c => c.text).join('\n')
                  : '',
              is_error: b.is_error || false,
            }));
        }
      }
    } catch {}
    return [];
  }

  /** Format a tool name for display — detect MCP tools */
  function fmtToolName(name) {
    if (!name) return '?';
    const mcp = name.match(/^mcp__([^_]+)__(.+)$/);
    if (mcp) return { server: mcp[1], tool: mcp[2], isMcp: true };
    return { tool: name, isMcp: false };
  }

  /** Summarize tool input — show most relevant fields */
  function summarizeToolInput(input) {
    if (!input || typeof input !== 'object') return '';
    // Common patterns
    if (input.command) return input.command;
    if (input.file_path) return input.file_path;
    if (input.pattern) return input.pattern + (input.path ? ` in ${input.path}` : '');
    if (input.query) return input.query;
    if (input.prompt) return input.prompt.length > 100 ? input.prompt.slice(0, 100) + '...' : input.prompt;
    if (input.content && typeof input.content === 'string') return input.content.length > 100 ? input.content.slice(0, 100) + '...' : input.content;
    // Generic: show first few key=value pairs
    const entries = Object.entries(input).slice(0, 3);
    return entries.map(([k, v]) => {
      const s = typeof v === 'string' ? v : JSON.stringify(v);
      return `${k}: ${s.length > 60 ? s.slice(0, 60) + '...' : s}`;
    }).join(', ');
  }

  /** Split user message into { userText, systemBlocks } */
  function splitUserQuery(requestBody) {
    if (!requestBody) return null;
    try {
      const body = JSON.parse(requestBody);
      if (!body.messages || !Array.isArray(body.messages)) return null;
      for (let i = body.messages.length - 1; i >= 0; i--) {
        const msg = body.messages[i];
        if (msg.role === 'user') {
          let raw;
          if (typeof msg.content === 'string') {
            raw = msg.content;
          } else if (Array.isArray(msg.content)) {
            raw = msg.content
              .filter(b => b.type === 'text')
              .map(b => b.text)
              .join('\n');
          }
          if (!raw) return null;

          // Collect system blocks
          const systemBlocks = [...raw.matchAll(SYSTEM_TAG_RE)].map(m => m[0]);

          // User text is everything outside system blocks
          let userText = raw.replace(SYSTEM_TAG_RE, '').replace(/\n{3,}/g, '\n\n').trim();

          return { userText: userText || null, systemBlocks };
        }
      }
    } catch { /* ignore parse errors */ }
    return null;
  }

  function redactAuth(headersStr) {
    if (!headersStr) return '';
    try {
      const h = JSON.parse(headersStr);
      const redacted = {};
      for (const [k, v] of Object.entries(h)) {
        redacted[k] = /^(x-api-key|authorization)$/i.test(k) ? '[REDACTED]' : v;
      }
      return JSON.stringify(redacted, null, 2);
    } catch { return headersStr; }
  }

  // Group OTEL events into turns by prompt.id
  let turns = $derived.by(() => {
    if (events.length === 0) return [];
    const evAsc = [...events].reverse();
    const turnMap = new Map(); // prompt.id -> { prompt, apiRequests, toolOps }
    const orphans = []; // events without prompt.id

    for (const ev of evAsc) {
      const attrs = parseAttrs(ev.attributes);
      const eventName = ev.name === 'log' ? (attrs['event.name'] || ev.name) : ev.name;
      const promptId = attrs['prompt.id'];

      const entry = { ev, attrs, eventName };

      if (!promptId) { orphans.push(entry); continue; }

      if (!turnMap.has(promptId)) {
        turnMap.set(promptId, { promptId, prompt: null, apiRequests: [], toolOps: [], firstTs: ev.timestamp });
      }
      const turn = turnMap.get(promptId);

      if (eventName === 'user_prompt') {
        turn.prompt = entry;
        turn.firstTs = ev.timestamp;
      } else if (eventName === 'api_request') {
        turn.apiRequests.push(entry);
      } else if (eventName === 'tool_decision' || eventName === 'tool_result') {
        turn.toolOps.push(entry);
      }
    }

    return [...turnMap.values()];
  });

  // Metrics derived values
  let maxToolUses = $derived(
    tools.reduce((m, t) => Math.max(m, Number(t.uses || 0)), 1)
  );
  let totals = $derived.by(() => {
    let input = 0, output = 0, cache = 0, latency = 0;
    for (const c of calls) {
      input += Number(c.input_tokens || 0);
      output += Number(c.output_tokens || 0);
      cache += Number(c.cache_read || 0);
      latency += Number(c.latency_ms || 0);
    }
    const totalInput = input + cache;
    return { input, output, cache, latency,
      cacheRate: totalInput > 0 ? (cache / totalInput * 100) : 0,
      avgLatency: calls.length > 0 ? latency / calls.length : 0,
    };
  });

  let stats = $derived(detail?.stats || {});
  let session = $derived(detail?.session || {});

  function classifyCall(call) {
    const label = { type: 'UNKNOWN', detail: null, tools: [] };

    let body = {};
    try { body = JSON.parse(call.request_body || '{}'); } catch {}

    // Tier 1: Type
    if (body.max_tokens === 1) {
      label.type = 'QUOTA';
      return label;
    }
    if ((call.path || '').includes('count_tokens')) {
      label.type = 'TOKEN COUNT';
      return label;
    }
    if (body.thinking) {
      label.type = 'MAIN';
    } else if (body.system && body.tools) {
      label.type = 'SUBAGENT';
      const sys = JSON.stringify(body.system);
      if (sys.includes('file search specialist') || sys.includes('READ-ONLY MODE')) {
        label.detail = 'EXPLORE';
      } else if (sys.includes('web search tool use')) {
        label.detail = 'WEB SEARCH';
      } else if (sys.includes('Claude Code Guide')) {
        label.detail = 'CC GUIDE';
      }
    }

    // Tier 2: Enrichment — extract tool_use names from assistant messages
    const msgs = body.messages || [];
    const toolCounts = {};
    for (const m of msgs) {
      if (m.role === 'assistant' && Array.isArray(m.content)) {
        for (const block of m.content) {
          if (block.type === 'tool_use' && block.name) {
            toolCounts[block.name] = (toolCounts[block.name] || 0) + 1;
          }
        }
      }
    }
    label.tools = Object.entries(toolCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 4);

    return label;
  }

  let callLabels = $derived.by(() => {
    let turnNum = 0;
    return calls.map(c => {
      const label = classifyCall(c);
      if (label.type === 'MAIN') {
        let body = {};
        try { body = JSON.parse(c.request_body || '{}'); } catch {}
        const msgs = body.messages || [];
        const lastUser = [...msgs].reverse().find(m => m.role === 'user');
        const hasToolResult = lastUser && Array.isArray(lastUser.content) &&
          lastUser.content.some(b => b.type === 'tool_result');
        if (hasToolResult) {
          // Continuation of current turn after tool results
          label.detail = turnNum > 0 ? `TURN ${turnNum}+` : 'TOOL LOOP';
        } else {
          turnNum++;
          label.detail = `TURN ${turnNum}`;
        }
      }
      return label;
    });
  });
</script>

<a class="back" href="#/">&larr; SESSIONS</a>

{#if loading}
  <div class="loading">Loading session...</div>
{:else if error}
  <div class="stub">Error loading session: {error}</div>
{:else if detail}
  <div class="stats-bar">
    <div class="stat">
      <div class="label">Duration</div>
      <div class="value">{fmtDuration(session.duration_secs)}</div>
    </div>
    <div class="stat">
      <div class="label">Model</div>
      <div class="value">{stats.model || '-'}</div>
    </div>
    <div class="stat">
      <div class="label">API Calls</div>
      <div class="value">{stats.api_calls ?? '-'}</div>
    </div>
    <div class="stat">
      <div class="label">Tokens In</div>
      <div class="value">{fmtTokens(stats.input_tokens)}</div>
    </div>
    <div class="stat">
      <div class="label">Tokens Out</div>
      <div class="value">{fmtTokens(stats.output_tokens)}</div>
    </div>
    <div class="stat">
      <div class="label">Cost</div>
      <div class="value">{fmtCost(stats.cost_usd)}</div>
    </div>
    <div class="stat">
      <div class="label">Files Changed</div>
      <div class="value">{session.files_changed ?? '-'}</div>
    </div>
  </div>

  <div class="tabs">
    <button class="tab" class:active={tab === 'proxy'} onclick={() => switchTab('proxy')}>PROXY</button>
    <button class="tab" class:active={tab === 'events'} onclick={() => switchTab('events')}>EVENTS</button>
    <button class="tab" class:active={tab === 'metrics'} onclick={() => switchTab('metrics')}>METRICS</button>
    <button class="tab" class:active={tab === 'insights'} onclick={() => switchTab('insights')}>INSIGHTS</button>
  </div>

  <!-- PROXY TAB: Full API traffic inspector -->
  {#if tab === 'proxy'}
    {#if calls.length === 0}
      <div class="stub">No proxy data</div>
    {:else}
      <div class="proxy-calls">
        {#each calls as c, i}
          {@const parsed = splitUserQuery(c.request_body)}
          {@const label = callLabels[i]}
          {@const toolCalls = parseToolCalls(c.response_tool_use)}
          {@const toolResults = extractToolResults(c.request_body)}
          {@const isNonConversation = label.type === 'QUOTA' || label.type === 'TOKEN COUNT'}
          {@const hasConversation = !isNonConversation && !!(parsed?.userText || parsed?.systemBlocks?.length || c.response_text || toolCalls.length || toolResults.length)}
          {@const viewMode = effectiveView(i, hasConversation)}
          <div class="proxy-card proxy-card-{label.type.toLowerCase().replace(' ', '-')}" class:proxy-card-error={c.status_code >= 400}>
            <div class="proxy-summary">
              <div class="proxy-summary-left">
                <span class="proxy-ts">{fmtTime(c.timestamp)}</span>
                <span class="proxy-type-pill proxy-type-{label.type.toLowerCase().replace(' ', '-')}">{label.detail || label.type}</span>
                {#if c.status_code >= 400}<span class="proxy-status" style="color: var(--fail)">{c.status_code}</span>{/if}
              </div>
              <div class="proxy-summary-center">
                <span class="proxy-model">{c.model || '-'}</span>
                <span class="proxy-latency">{fmtMs(c.latency_ms)}</span>
              </div>
              <div class="proxy-summary-right">
                <span class="proxy-token-group">
                  <span class="proxy-tokens">{fmtTokens(c.input_tokens)} in / {fmtTokens(c.output_tokens)} out</span>
                  {#if c.cache_read}<span class="proxy-cache">cache: {fmtTokens(c.cache_read)}</span>{/if}
                </span>
                {#if label.tools.length > 0}
                  <span class="proxy-tool-tags">{label.tools.map(([name, count]) => count > 1 ? `${name} ×${count}` : name).join(' · ')}</span>
                {/if}
                {#if hasConversation}
                  <button class="proxy-view-btn" class:active={viewMode === 'conv'} onclick={(e) => setCallView(i, 'conv', e)}>CONV</button>
                {/if}
                <button class="proxy-view-btn" class:active={viewMode === 'inspect'} onclick={(e) => setCallView(i, 'inspect', e)}>INSPECT</button>
                <button class="proxy-view-btn" class:active={viewMode === 'raw'} onclick={(e) => setCallView(i, 'raw', e)}>RAW</button>
              </div>
            </div>

            {#if viewMode === 'conv' && hasConversation}
              <div class="proxy-conversation">
                <!-- Tool results from previous call -->
                {#if toolResults.length > 0}
                  {#each toolResults as tr}
                    <div class="proxy-msg proxy-msg-tool-result" class:proxy-msg-tool-error={tr.is_error}>
                      <span class="proxy-msg-label proxy-msg-label-tool-result">TOOL RESULT{#if tr.is_error} (ERROR){/if}</span>
                      {#if tr.content}
                        <div class="proxy-msg-text proxy-tool-output">{tr.content.length > 500 ? tr.content.slice(0, 500) + '...' : tr.content}</div>
                      {/if}
                    </div>
                  {/each}
                {/if}

                <!-- User/instruction message -->
                {#if parsed?.userText || parsed?.systemBlocks?.length}
                  <div class="proxy-msg proxy-msg-user">
                    <span class="proxy-msg-label {label.type === 'SUBAGENT' ? 'proxy-msg-label-agent' : 'proxy-msg-label-user'}">
                      {label.type === 'SUBAGENT' ? 'INSTRUCTION' : 'YOU'}
                    </span>
                    {#if parsed.userText}
                      <div class="proxy-msg-text markdown">{@html renderMarkdown(parsed.userText)}</div>
                    {/if}
                    {#if parsed.systemBlocks.length > 0}
                      <details class="proxy-system-blocks">
                        <summary>{parsed.systemBlocks.length} system {parsed.systemBlocks.length === 1 ? 'block' : 'blocks'} injected</summary>
                        <div class="proxy-system-blocks-inner">
                          {#each parsed.systemBlocks as block}
                            {@const rendered = renderSystemBlock(block)}
                            <div class="proxy-system-block">
                              <span class="proxy-system-tag">&lt;{rendered.tag}&gt;</span>
                              <div class="proxy-system-content markdown">{@html rendered.html}</div>
                            </div>
                          {/each}
                        </div>
                      </details>
                    {/if}
                  </div>
                {/if}

                <!-- Model response text -->
                {#if c.response_text}
                  <div class="proxy-msg proxy-msg-model">
                    <span class="proxy-msg-label proxy-msg-label-model">MODEL</span>
                    <div class="proxy-msg-text markdown">{@html renderMarkdown(c.response_text)}</div>
                  </div>
                {/if}

                <!-- Tool calls from model response -->
                {#if toolCalls.length > 0}
                  {#each toolCalls as tc}
                    {@const tn = fmtToolName(tc.name)}
                    <div class="proxy-msg proxy-msg-tool-call">
                      <span class="proxy-msg-label proxy-msg-label-tool-call">
                        {#if tn.isMcp}MCP{:else}TOOL{/if}
                      </span>
                      <div class="proxy-tool-call-content">
                        <span class="proxy-tool-name">{#if tn.isMcp}<span class="proxy-tool-server">{tn.server}/</span>{/if}{tn.tool}</span>
                        {#if summarizeToolInput(tc.input)}
                          <span class="proxy-tool-input">{summarizeToolInput(tc.input)}</span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                {/if}
              </div>
            {/if}

            {#if viewMode === 'inspect'}
              {@const inspected = parseRequestBody(c.request_body)}
              {#if !inspected}
                <div class="stub">No request body to inspect</div>
              {:else}
                <div class="inspect-view">
                  <!-- Parameters Strip -->
                  <div class="inspect-params">
                    {#if inspected.model}<span class="inspect-pill"><strong>{inspected.model}</strong></span>{/if}
                    {#if inspected.max_tokens}<span class="inspect-pill">max_tokens: <strong>{inspected.max_tokens.toLocaleString()}</strong></span>{/if}
                    {#if inspected.thinking}<span class="inspect-pill" class:inspect-pill-on={inspected.thinking.type !== 'disabled'}>thinking: <strong>{inspected.thinking.type || inspected.thinking}</strong></span>{/if}
                    {#if inspected.effort}<span class="inspect-pill">effort: <strong>{inspected.effort}</strong></span>{/if}
                    {#if inspected.stream != null}<span class="inspect-pill">stream: <strong>{inspected.stream}</strong></span>{/if}
                  </div>

                  <!-- System Blocks -->
                  {#if inspected.system.length > 0}
                    <div class="inspect-section">
                      <div class="section-title">System ({inspected.system.length})</div>
                      {#each inspected.system as sysBlock, si}
                        <details class="inspect-system-block">
                          <summary>
                            <span class="inspect-system-label">System #{si + 1}</span>
                            {#if sysBlock.cache_control}<span class="inspect-cache-pill">cached</span>{/if}
                            <span class="inspect-system-preview">{sysBlock.preview}</span>
                          </summary>
                          <div class="inspect-system-body">{sysBlock.text}</div>
                        </details>
                      {/each}
                    </div>
                  {/if}

                  <!-- Messages -->
                  {#if inspected.messages.length > 0}
                    <div class="inspect-section">
                      <div class="section-title">Messages ({inspected.messages.length})</div>
                      {#each inspected.messages as msg}
                        <details class="inspect-msg"
                          class:inspect-msg-has-tool-use={msg.blockCounts.tool_use > 0}
                          class:inspect-msg-has-tool-result={msg.blockCounts.tool_result > 0}>
                          <summary>
                            <span class="inspect-msg-index">#{msg.index + 1}</span>
                            <span class="proxy-msg-label {msg.role === 'user' ? 'proxy-msg-label-user' : 'proxy-msg-label-model'}">{msg.role}</span>
                            <span class="inspect-block-count">
                              {#each Object.entries(msg.blockCounts).filter(([,v]) => v > 0) as [type, count]}
                                {count} {type}{' '}
                              {/each}
                            </span>
                            {#if msg.toolUseNames.length > 0}
                              <span class="inspect-msg-tools">({msg.toolUseNames.join(', ')})</span>
                            {/if}
                            <span class="inspect-msg-tokens">~{fmtTokens(msg.estimatedTokens)}</span>
                            {#if msg.textPreview}
                              <span class="inspect-msg-preview">{msg.textPreview}</span>
                            {/if}
                          </summary>
                          <div class="inspect-msg-body">
                            {#each msg.blocks as block}
                              <div class="inspect-msg-block">
                                {#if block.type === 'text'}
                                  <div class="inspect-text-block markdown">{@html renderMarkdown(block.text || '')}</div>
                                {:else if block.type === 'thinking'}
                                  <div class="inspect-thinking">{block.thinking || ''}</div>
                                {:else if block.type === 'tool_use'}
                                  <div class="inspect-tool-use-block">
                                    <span class="proxy-msg-label proxy-msg-label-tool-call">TOOL</span>
                                    <strong>{block.name}</strong>
                                    {#if block.input}
                                      <span class="proxy-tool-input">{summarizeToolInput(block.input)}</span>
                                    {/if}
                                  </div>
                                {:else if block.type === 'tool_result'}
                                  <div class="inspect-tool-result-block" class:inspect-tool-result-error={block.is_error}>
                                    <span class="proxy-msg-label proxy-msg-label-tool-result">RESULT{#if block.is_error} (ERROR){/if}</span>
                                    {#if typeof block.content === 'string'}
                                      {block.content.length > 500 ? block.content.slice(0, 500) + '...' : block.content}
                                    {:else if Array.isArray(block.content)}
                                      {block.content.filter(b => b.type === 'text').map(b => b.text).join('\n').slice(0, 500)}
                                    {/if}
                                  </div>
                                {/if}
                              </div>
                            {/each}
                          </div>
                        </details>
                      {/each}
                    </div>
                  {/if}

                  <!-- Tools Inventory -->
                  {#if inspected.tools.length > 0}
                    <div class="inspect-section">
                      <div class="section-title">Tools ({inspected.tools.length})</div>
                      {#each [...inspected.toolGroups.entries()] as [groupKey, groupTools]}
                        <div class="inspect-tool-group">
                          <div class="inspect-tool-group-label">
                            {#if groupKey === 'native' || groupKey === 'builtin'}
                              {groupKey} ({groupTools.length})
                            {:else}
                              MCP: {groupKey} ({groupTools.length})
                            {/if}
                          </div>
                          <div class="inspect-tool-pills">
                            {#each groupTools as t}
                              <details class="inspect-tool-pill">
                                <summary>{t.displayName}</summary>
                                <div class="inspect-tool-detail">
                                  {#if t.description}<div class="inspect-tool-desc">{t.description}</div>{/if}
                                  {#if t.input_schema}<pre class="json-hl">{@html prettyJsonHtml(JSON.stringify(t.input_schema))}</pre>{/if}
                                </div>
                              </details>
                            {/each}
                          </div>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            {/if}

            {#if viewMode === 'raw'}
              <div class="proxy-details">
                <div class="proxy-details-label">Raw</div>
                {#if c.request_headers}
                  <details class="proxy-section">
                    <summary>Request Headers</summary>
                    <pre class="json-hl">{@html prettyJsonHtml(redactAuth(c.request_headers))}</pre>
                  </details>
                {/if}
                {#if c.response_headers}
                  <details class="proxy-section">
                    <summary>Response Headers</summary>
                    <pre class="json-hl">{@html prettyJsonHtml(c.response_headers)}</pre>
                  </details>
                {/if}
                {#if c.request_body}
                  <details class="proxy-section">
                    <summary>Request Body</summary>
                    <pre class="json-hl">{@html prettyJsonHtml(c.request_body)}</pre>
                  </details>
                {/if}
                {#if c.response_body}
                  <details class="proxy-section">
                    <summary>Response Body</summary>
                    <pre class="json-hl">{@html formatResponseBodyHtml(c.response_body)}</pre>
                  </details>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

  <!-- EVENTS TAB: OTEL telemetry grouped by turn -->
  {:else if tab === 'events'}
    {#if turns.length === 0}
      <div class="stub">No OTEL events</div>
    {:else}
      <div class="turns">
        {#each turns as turn, ti}
          <div class="turn">
            <!-- User prompt -->
            {#if turn.prompt}
              {@const promptKey = `${ti}-prompt-0`}
              <div class="turn-prompt">
                <div class="turn-prompt-header event-expandable" onclick={() => toggleEvent(promptKey)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && toggleEvent(promptKey)}>
                  <span class="turn-number">#{ti + 1}</span>
                  <span class="turn-label prompt-label">YOU</span>
                  <span class="turn-ts">{fmtTime(turn.prompt.ev.timestamp)}</span>
                  {#if turn.prompt.attrs.prompt_length}
                    <span class="turn-meta">{turn.prompt.attrs.prompt_length} chars</span>
                  {/if}
                  <span class="event-expand-arrow">{expandedEvents[promptKey] ? '▼' : '▶'}</span>
                </div>
                <div class="turn-prompt-text">{turn.prompt.attrs.prompt || '(empty)'}</div>
                {#if expandedEvents[promptKey]}
                  <div class="event-details">
                    <details class="event-raw-attrs" open>
                      <summary>Raw Attributes</summary>
                      <pre class="json-hl">{@html highlightJson(rawAttrsJson(turn.prompt.attrs))}</pre>
                    </details>
                  </div>
                {/if}
              </div>
            {/if}

            <!-- API requests for this turn -->
            {#each turn.apiRequests as req, ri}
              {@const apiKey = `${ti}-api-${ri}`}
              <div class="turn-api">
                <div class="turn-api-header event-expandable" onclick={() => toggleEvent(apiKey)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && toggleEvent(apiKey)}>
                  <span class="turn-label api-label">API</span>
                  <span class="turn-ts">{fmtTime(req.ev.timestamp)}</span>
                  <span class="turn-model">{req.attrs.model || '-'}</span>
                  <span class="turn-meta">{fmtMs(req.attrs.duration_ms)}</span>
                  <span class="event-expand-arrow">{expandedEvents[apiKey] ? '▼' : '▶'}</span>
                </div>
                <div class="turn-api-stats">
                  <span class="tok-pill input">{fmtTokens(req.attrs.input_tokens)} in</span>
                  <span class="tok-pill output">{fmtTokens(req.attrs.output_tokens)} out</span>
                  {#if Number(req.attrs.cache_read_tokens) > 0}
                    <span class="tok-pill cache">{fmtTokens(req.attrs.cache_read_tokens)} cache</span>
                  {/if}
                  {#if req.attrs.cost_usd}
                    <span class="tok-pill cost">{fmtCost(req.attrs.cost_usd)}</span>
                  {/if}
                  {#if req.attrs.speed}
                    <span class="turn-meta">speed: {req.attrs.speed}</span>
                  {/if}
                </div>
                {#if expandedEvents[apiKey]}
                  <div class="event-details">
                    <div class="event-key-fields">
                      {#each eventKeyFields('api_request', req.attrs) as f}
                        <span class="event-key-label">{f.label}</span>
                        <span class="event-key-value">{f.value}</span>
                      {/each}
                    </div>
                    <details class="event-raw-attrs">
                      <summary>Raw Attributes</summary>
                      <pre class="json-hl">{@html highlightJson(rawAttrsJson(req.attrs))}</pre>
                    </details>
                  </div>
                {/if}
              </div>
            {/each}

            <!-- Tool operations for this turn -->
            {#if turn.toolOps.length > 0}
              <div class="turn-tools">
                {#each turn.toolOps as op, oi}
                  {@const opKey = `${ti}-tool-${oi}`}
                  {#if op.eventName === 'tool_decision'}
                    <div class="tool-op tool-decision-op">
                      <div class="tool-op-row event-expandable" onclick={() => toggleEvent(opKey)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && toggleEvent(opKey)}>
                        <span class="tool-op-icon">{op.attrs.decision === 'accept' || op.attrs.decision === 'approved' ? '>' : 'x'}</span>
                        <span class="tool-op-name">{op.attrs.tool_name || '-'}</span>
                        <span class="tool-op-detail">
                          {op.attrs.decision || '-'}
                          <span class="tool-op-source">({op.attrs.source || '-'})</span>
                        </span>
                        <span class="event-expand-arrow">{expandedEvents[opKey] ? '▼' : '▶'}</span>
                      </div>
                      {#if expandedEvents[opKey]}
                        <div class="event-details">
                          <div class="event-key-fields">
                            {#each eventKeyFields('tool_decision', op.attrs) as f}
                              <span class="event-key-label">{f.label}</span>
                              <span class="event-key-value">{f.value}</span>
                            {/each}
                          </div>
                          <details class="event-raw-attrs">
                            <summary>Raw Attributes</summary>
                            <pre class="json-hl">{@html highlightJson(rawAttrsJson(op.attrs))}</pre>
                          </details>
                        </div>
                      {/if}
                    </div>
                  {:else if op.eventName === 'tool_result'}
                    <div class="tool-op tool-result-op {op.attrs.success === 'true' ? 'tool-ok' : 'tool-fail'}">
                      <div class="tool-op-row event-expandable" onclick={() => toggleEvent(opKey)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && toggleEvent(opKey)}>
                        <span class="tool-op-icon">{op.attrs.success === 'true' ? '+' : '!'}</span>
                        <span class="tool-op-name">{op.attrs.tool_name || '-'}</span>
                        <span class="tool-op-detail">
                          <span style="color: {op.attrs.success === 'true' ? 'var(--ok)' : 'var(--fail)'}; font-weight: 600">
                            {op.attrs.success === 'true' ? 'OK' : 'FAIL'}
                          </span>
                          {#if op.attrs.duration_ms}
                            &middot; {fmtMs(op.attrs.duration_ms)}
                          {/if}
                          {#if op.attrs.tool_result_size_bytes}
                            &middot; {Number(op.attrs.tool_result_size_bytes) >= 1000 ? `${(Number(op.attrs.tool_result_size_bytes)/1000).toFixed(1)}KB` : `${op.attrs.tool_result_size_bytes}B`}
                          {/if}
                        </span>
                        <span class="event-expand-arrow">{expandedEvents[opKey] ? '▼' : '▶'}</span>
                      </div>
                      {#if op.attrs.error}
                        <div class="tool-op-error">{op.attrs.error}</div>
                      {/if}
                      {#if expandedEvents[opKey]}
                        <div class="event-details">
                          <div class="event-key-fields">
                            {#each eventKeyFields('tool_result', op.attrs) as f}
                              <span class="event-key-label">{f.label}</span>
                              <span class="event-key-value">{f.value}</span>
                            {/each}
                          </div>
                          {#if op.attrs.tool_parameters}
                            <details class="event-raw-attrs">
                              <summary>Parameters</summary>
                              <pre class="json-hl">{@html prettyJsonHtml(op.attrs.tool_parameters)}</pre>
                            </details>
                          {/if}
                          <details class="event-raw-attrs">
                            <summary>Raw Attributes</summary>
                            <pre class="json-hl">{@html highlightJson(rawAttrsJson(op.attrs))}</pre>
                          </details>
                        </div>
                      {/if}
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

  <!-- METRICS TAB -->
  {:else if tab === 'metrics'}
    {#if calls.length === 0 && tools.length === 0}
      <div class="stub">No metrics data</div>
    {:else}
      <!-- Summary cards -->
      <div class="metrics-cards">
        <div class="metrics-card">
          <div class="metrics-card-label">Total Cost</div>
          <div class="metrics-card-value">{fmtCost(stats.cost_usd)}</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">API Calls</div>
          <div class="metrics-card-value">{calls.length}</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">Avg Latency</div>
          <div class="metrics-card-value">{fmtMs(totals.avgLatency)}</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">Cache Hit Rate</div>
          <div class="metrics-card-value">{totals.cacheRate.toFixed(0)}%</div>
        </div>
      </div>

      <!-- Token breakdown bar -->
      <div class="section-title">Token Breakdown</div>
      <div class="bar-legend">
        <span class="l-input">Input ({fmtTokens(totals.input)})</span>
        <span class="l-output">Output ({fmtTokens(totals.output)})</span>
        <span class="l-cache">Cache Read ({fmtTokens(totals.cache)})</span>
      </div>
      {@const tokenTotal = totals.input + totals.output + totals.cache || 1}
      <div class="stacked-bar">
        <div class="stacked-seg input" style="width: {totals.input / tokenTotal * 100}%"
             title="Input: {fmtTokens(totals.input)}"></div>
        <div class="stacked-seg output" style="width: {totals.output / tokenTotal * 100}%"
             title="Output: {fmtTokens(totals.output)}"></div>
        <div class="stacked-seg cache" style="width: {totals.cache / tokenTotal * 100}%"
             title="Cache: {fmtTokens(totals.cache)}"></div>
      </div>

      <!-- API calls table -->
      <div class="section-title">API Calls</div>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>Model</th>
            <th class="num">Input</th>
            <th class="num">Output</th>
            <th class="num">Cache</th>
            <th class="num">Latency</th>
          </tr>
        </thead>
        <tbody>
          {#each calls as c, i}
            <tr>
              <td>{i + 1}</td>
              <td>{c.model || '-'}</td>
              <td class="num">{fmtTokens(c.input_tokens)}</td>
              <td class="num">{fmtTokens(c.output_tokens)}</td>
              <td class="num">{fmtTokens(c.cache_read)}</td>
              <td class="num">{fmtMs(c.latency_ms)}</td>
            </tr>
          {/each}
          {#if calls.length > 1}
            <tr class="totals-row">
              <td></td>
              <td>Total</td>
              <td class="num">{fmtTokens(totals.input)}</td>
              <td class="num">{fmtTokens(totals.output)}</td>
              <td class="num">{fmtTokens(totals.cache)}</td>
              <td class="num">{fmtMs(totals.latency)}</td>
            </tr>
          {/if}
        </tbody>
      </table>

      <!-- Tool usage -->
      {#if tools.length > 0}
        <div class="section-title">Tool Usage</div>
        <table>
          <thead>
            <tr>
              <th>Tool</th>
              <th class="num">Uses</th>
              <th class="num">OK</th>
              <th class="num">Fail</th>
              <th style="width: 40%"></th>
            </tr>
          </thead>
          <tbody>
            {#each tools as t}
              <tr>
                <td>{t.tool_name}</td>
                <td class="num">{t.uses}</td>
                <td class="num" style="color: var(--ok)">{t.successes}</td>
                <td class="num" style="color: var(--fail)">{t.failures}</td>
                <td>
                  <div class="bar-track" style="height: 12px">
                    <div class="bar-fill" style="width: {pct(t.uses, maxToolUses)}%; background: var(--fg)"></div>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

  {:else if tab === 'insights'}
    {#if !insights}
      <div class="loading">Loading insights...</div>
    {:else}
      <!-- Cache Analysis -->
      <div class="section-title">Cache Analysis</div>
      <div class="metrics-cards">
        <div class="metrics-card">
          <div class="metrics-card-label">Cache Hit Rate</div>
          <div class="metrics-card-value">{(insights.cache_analysis.cache_hit_rate * 100).toFixed(0)}%</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">Est. Savings</div>
          <div class="metrics-card-value" style="color: var(--ok)">${insights.cache_analysis.estimated_savings_usd.toFixed(4)}</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">Cache Read</div>
          <div class="metrics-card-value">{fmtTokens(insights.cache_analysis.total_cache_read)}</div>
        </div>
        <div class="metrics-card">
          <div class="metrics-card-label">Cache Write</div>
          <div class="metrics-card-value">{fmtTokens(insights.cache_analysis.total_cache_write)}</div>
        </div>
      </div>

      <!-- Stop Reasons -->
      {#if Object.keys(insights.stop_reasons).length > 0}
        <div class="section-title">Stop Reasons</div>
        <div class="insights-pills">
          {#each Object.entries(insights.stop_reasons) as [reason, count]}
            <span class="insights-pill" class:insights-pill-warn={reason === 'max_tokens'}>
              {reason} <strong>{count}</strong>
            </span>
          {/each}
        </div>
      {/if}

      <!-- System Prompt -->
      {#if insights.system_prompt_tokens}
        <div class="section-title">System Prompt</div>
        <div class="metrics-cards">
          <div class="metrics-card">
            <div class="metrics-card-label">Est. Size</div>
            <div class="metrics-card-value">~{fmtTokens(insights.system_prompt_tokens)} tokens</div>
          </div>
        </div>
      {/if}

      <!-- Context Growth -->
      {#if insights.context_growth.length > 0}
        <div class="section-title">Context Growth</div>
        {@const maxInput = Math.max(...insights.context_growth.map(p => p.input_tokens)) || 1}
        <div class="context-chart">
          {#each insights.context_growth as point}
            <div class="context-bar-row">
              <div class="context-bar-label">#{point.call_number}</div>
              <div class="bar-track">
                <div class="bar-fill input" style="width: {point.input_tokens / maxInput * 100}%"
                     title="{fmtTokens(point.input_tokens)} input tokens"></div>
              </div>
              <div class="context-bar-value">{fmtTokens(point.input_tokens)}</div>
              {#if point.stop_reason}
                <span class="insights-pill-sm" class:insights-pill-warn={point.stop_reason === 'max_tokens'}>
                  {point.stop_reason}
                </span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <!-- Rate Limits -->
      {#if insights.rate_limits.length > 0}
        <div class="section-title">Rate Limits</div>
        {@const lastRL = insights.rate_limits[insights.rate_limits.length - 1]}
        <div class="metrics-cards">
          {#if lastRL.requests_remaining != null}
            <div class="metrics-card">
              <div class="metrics-card-label">Requests Remaining</div>
              <div class="metrics-card-value" class:insights-warn={lastRL.requests_remaining < 10}>
                {lastRL.requests_remaining}{#if lastRL.requests_limit} / {lastRL.requests_limit}{/if}
              </div>
            </div>
          {/if}
          {#if lastRL.tokens_remaining != null}
            <div class="metrics-card">
              <div class="metrics-card-label">Tokens Remaining</div>
              <div class="metrics-card-value" class:insights-warn={lastRL.tokens_remaining < 10000}>
                {fmtTokens(lastRL.tokens_remaining)}{#if lastRL.tokens_limit} / {fmtTokens(lastRL.tokens_limit)}{/if}
              </div>
            </div>
          {/if}
        </div>

        <!-- Rate limit over time -->
        {#if insights.rate_limits.length > 1}
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th class="num">Req Remaining</th>
                <th class="num">Token Remaining</th>
              </tr>
            </thead>
            <tbody>
              {#each insights.rate_limits as rl}
                <tr>
                  <td>{fmtTime(rl.timestamp)}</td>
                  <td class="num" class:insights-warn={rl.requests_remaining != null && rl.requests_remaining < 10}>
                    {rl.requests_remaining ?? '-'}{#if rl.requests_limit} / {rl.requests_limit}{/if}
                  </td>
                  <td class="num" class:insights-warn={rl.tokens_remaining != null && rl.tokens_remaining < 10000}>
                    {rl.tokens_remaining != null ? fmtTokens(rl.tokens_remaining) : '-'}{#if rl.tokens_limit} / {fmtTokens(rl.tokens_limit)}{/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    {/if}
  {/if}
{/if}
