<script>
  import { onMount } from 'svelte';
  import { marked } from 'marked';
  import {
    fetchSession, fetchEvents, fetchCalls, fetchTools,
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
  let expandedCalls = $state({});

  onMount(async () => {
    try {
      const [d, c] = await Promise.all([
        fetchSession(id),
        fetchCalls(id),
      ]);
      detail = d;
      calls = c;
    } catch (e) {
      console.error('Failed to load session:', e);
    } finally {
      loading = false;
    }
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

  function switchTab(t) {
    tab = t;
    if (t === 'events') loadEvents();
    if (t === 'metrics') loadMetrics();
  }

  function toggleCall(i) {
    expandedCalls = { ...expandedCalls, [i]: !expandedCalls[i] };
  }

  function tryPrettyJson(str) {
    if (!str) return '';
    try { return JSON.stringify(JSON.parse(str), null, 2); } catch { return str; }
  }

  /** Syntax-highlight a JSON string into HTML spans */
  function highlightJson(str) {
    if (!str) return '';
    return str
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"([^"\\]*(\\.[^"\\]*)*)"(\s*:)?/g, (match, key, _esc, colon) => {
        if (colon) return `<span class="json-key">"${key}"</span>:`;
        return `<span class="json-str">"${key}"</span>`;
      })
      .replace(/\b(-?\d+\.?\d*([eE][+-]?\d+)?)\b/g, '<span class="json-num">$1</span>')
      .replace(/\b(true|false)\b/g, '<span class="json-bool">$1</span>')
      .replace(/\bnull\b/g, '<span class="json-null">null</span>');
  }

  /** Pretty-print + highlight a JSON string */
  function prettyJsonHtml(str) {
    if (!str) return '';
    try {
      return highlightJson(JSON.stringify(JSON.parse(str), null, 2));
    } catch {
      return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }
  }

  /** Format SSE stream or JSON response body with highlighting */
  function formatResponseBodyHtml(str) {
    if (!str) return '';
    // Plain JSON
    try {
      const parsed = JSON.parse(str);
      return highlightJson(JSON.stringify(parsed, null, 2));
    } catch { /* not plain JSON, try SSE */ }
    // SSE: highlight each event block
    const escaped = str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    return escaped.replace(/^(event: .+)$/gm, '<span class="sse-event">$1</span>')
      .replace(/^data: (.+)$/gm, (_match, json) => {
        // json is already HTML-escaped, unescape for parse then re-highlight
        const raw = json.replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>');
        try {
          return '<span class="sse-data">data: </span>' + highlightJson(JSON.stringify(JSON.parse(raw), null, 2));
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
          const systemBlocks = [];
          let match;
          const re = new RegExp(SYSTEM_TAG_RE.source, 'g');
          while ((match = re.exec(raw)) !== null) {
            systemBlocks.push(match[0]);
          }

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
</script>

<a class="back" href="#/">&larr; SESSIONS</a>

{#if loading}
  <div class="loading">Loading session...</div>
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
  </div>

  <!-- PROXY TAB: Full API traffic inspector -->
  {#if tab === 'proxy'}
    {#if calls.length === 0}
      <div class="stub">No proxy data</div>
    {:else}
      <div class="proxy-calls">
        {#each calls as c, i}
          {@const parsed = splitUserQuery(c.request_body)}
          <div class="proxy-card">
            <div class="proxy-summary" onclick={() => toggleCall(i)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && toggleCall(i)}>
              <span class="proxy-ts">{fmtTime(c.timestamp)}</span>
              <span class="proxy-method">{c.method}</span>
              <span class="proxy-path">{c.path}</span>
              <span class="proxy-status" style="color: {c.status_code < 400 ? 'var(--ok)' : 'var(--fail)'}">{c.status_code ?? '-'}</span>
              <span class="proxy-model">{c.model || '-'}</span>
              <span class="proxy-latency">{fmtMs(c.latency_ms)}</span>
              <span class="proxy-tokens">{fmtTokens(c.input_tokens)} in / {fmtTokens(c.output_tokens)} out</span>
              {#if c.cache_read}<span class="proxy-cache">cache: {fmtTokens(c.cache_read)}</span>{/if}
              <span class="proxy-expand">{expandedCalls[i] ? '▼' : '▶'}</span>
            </div>

            {#if parsed?.userText || parsed?.systemBlocks.length || c.response_text}
              <div class="proxy-conversation">
                {#if parsed?.userText || parsed?.systemBlocks.length}
                  <div class="proxy-msg proxy-msg-user">
                    <span class="proxy-msg-label proxy-msg-label-user">YOU</span>
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
                {#if c.response_text}
                  <div class="proxy-msg proxy-msg-model">
                    <span class="proxy-msg-label proxy-msg-label-model">MODEL</span>
                    <div class="proxy-msg-text markdown">{@html renderMarkdown(c.response_text)}</div>
                  </div>
                {/if}
              </div>
            {/if}

            {#if expandedCalls[i]}
              <div class="proxy-details">
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
              <div class="turn-prompt">
                <div class="turn-prompt-header">
                  <span class="turn-number">#{ti + 1}</span>
                  <span class="turn-label prompt-label">YOU</span>
                  <span class="turn-ts">{fmtTime(turn.prompt.ev.timestamp)}</span>
                  {#if turn.prompt.attrs.prompt_length}
                    <span class="turn-meta">{turn.prompt.attrs.prompt_length} chars</span>
                  {/if}
                </div>
                <div class="turn-prompt-text">{turn.prompt.attrs.prompt || '(empty)'}</div>
              </div>
            {/if}

            <!-- API requests for this turn -->
            {#each turn.apiRequests as req}
              <div class="turn-api">
                <div class="turn-api-header">
                  <span class="turn-label api-label">API</span>
                  <span class="turn-ts">{fmtTime(req.ev.timestamp)}</span>
                  <span class="turn-model">{req.attrs.model || '-'}</span>
                  <span class="turn-meta">{fmtMs(req.attrs.duration_ms)}</span>
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
              </div>
            {/each}

            <!-- Tool operations for this turn -->
            {#if turn.toolOps.length > 0}
              <div class="turn-tools">
                {#each turn.toolOps as op}
                  {#if op.eventName === 'tool_decision'}
                    <div class="tool-op tool-decision-op">
                      <span class="tool-op-icon">{op.attrs.decision === 'accept' || op.attrs.decision === 'approved' ? '>' : 'x'}</span>
                      <span class="tool-op-name">{op.attrs.tool_name || '-'}</span>
                      <span class="tool-op-detail">
                        {op.attrs.decision || '-'}
                        <span class="tool-op-source">({op.attrs.source || '-'})</span>
                      </span>
                    </div>
                  {:else if op.eventName === 'tool_result'}
                    <div class="tool-op tool-result-op {op.attrs.success === 'true' ? 'tool-ok' : 'tool-fail'}">
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
                      {#if op.attrs.error}
                        <div class="tool-op-error">{op.attrs.error}</div>
                      {/if}
                      {#if op.attrs.tool_parameters}
                        <details class="tool-op-params">
                          <summary>parameters</summary>
                          <pre>{tryPrettyJson(op.attrs.tool_parameters)}</pre>
                        </details>
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
  {/if}
{/if}
