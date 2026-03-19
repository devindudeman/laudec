<script>
  import { onMount } from 'svelte';
  import { fetchSessions, fmtDuration, fmtTokens, fmtCost, fmtDatetime } from '../lib/api.js';

  let sessions = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let timer = null;

  async function load() {
    try {
      sessions = await fetchSessions();
      error = null;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    timer = setInterval(load, 10000);
    return () => clearInterval(timer);
  });

  function navigate(id) {
    window.location.hash = `#/session/${id}`;
  }

  function fmtModel(model) {
    if (!model) return '-';
    return model.replace(/^claude-/, '');
  }

  function costClass(cost) {
    if (cost == null || cost < 0.50) return 'cost-low';
    if (cost < 2.0) return 'cost-mid';
    return 'cost-high';
  }

  function sessionTitle(s) {
    const text = s.first_prompt || s.summary || '-';
    return text.length > 80 ? text.slice(0, 80) + '...' : text;
  }

  let totals = $derived(() => {
    let calls = 0, cost = 0, errors = 0, totalDur = 0;
    for (const s of sessions) {
      calls += s.api_call_count ?? 0;
      cost += s.cost_usd ?? 0;
      errors += s.error_count ?? 0;
      totalDur += s.duration_secs ?? 0;
    }
    return { count: sessions.length, calls, cost, errors, duration: totalDur };
  });
</script>

{#if loading}
  <div class="loading">Loading sessions...</div>
{:else if error}
  <div class="stub">Error loading sessions: {error}</div>
{:else if sessions.length === 0}
  <div class="stub">No sessions recorded yet</div>
{:else}
  <table>
    <thead>
      <tr>
        <th>TIME</th>
        <th>DUR</th>
        <th>PROJECT</th>
        <th>MODEL</th>
        <th class="num">CALLS</th>
        <th class="num">COST</th>
        <th>TITLE</th>
      </tr>
    </thead>
    <tbody>
      {#each sessions as s}
        <tr class="clickable" onclick={() => navigate(s.id)}>
          <td>
            {fmtDatetime(s.started_at)}
            {#if s.project === 'active'}
              <span class="badge-live">LIVE</span>
            {/if}
          </td>
          <td>{fmtDuration(s.duration_secs)}</td>
          <td>{s.project || '-'}</td>
          <td>{fmtModel(s.model)}</td>
          <td class="num">
            {s.api_call_count ?? '-'}
            {#if s.error_count > 0}
              <span class="err-badge">{s.error_count} err</span>
            {/if}
          </td>
          <td class="num {costClass(s.cost_usd)}">{fmtCost(s.cost_usd)}</td>
          <td class="summary">{sessionTitle(s)}</td>
        </tr>
      {/each}
    </tbody>
    <tfoot>
      <tr class="totals-row">
        <td colspan="2">{totals().count} sessions · {fmtDuration(totals().duration)}</td>
        <td></td>
        <td></td>
        <td class="num">
          {totals().calls}
          {#if totals().errors > 0}
            <span class="err-badge">{totals().errors} err</span>
          {/if}
        </td>
        <td class="num {costClass(totals().cost)}">{fmtCost(totals().cost)}</td>
        <td></td>
      </tr>
    </tfoot>
  </table>
{/if}
