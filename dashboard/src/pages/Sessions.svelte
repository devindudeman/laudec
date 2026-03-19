<script>
  import { onMount } from 'svelte';
  import { fetchSessions, fmtDuration, fmtTokens, fmtCost, fmtDatetime } from '../lib/api.js';

  let sessions = $state([]);
  let loading = $state(true);
  let timer = null;

  async function load() {
    try {
      sessions = await fetchSessions();
    } catch (e) {
      console.error('Failed to fetch sessions:', e);
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
</script>

{#if loading}
  <div class="loading">Loading sessions...</div>
{:else if sessions.length === 0}
  <div class="stub">No sessions recorded yet</div>
{:else}
  <table>
    <thead>
      <tr>
        <th>TIME</th>
        <th>DURATION</th>
        <th>PROJECT</th>
        <th class="num">CALLS</th>
        <th class="num">IN</th>
        <th class="num">OUT</th>
        <th class="num">COST</th>
        <th class="num">FILES</th>
        <th>SUMMARY</th>
      </tr>
    </thead>
    <tbody>
      {#each sessions as s}
        <tr class="clickable" onclick={() => navigate(s.id)}>
          <td>{fmtDatetime(s.started_at)}</td>
          <td>{fmtDuration(s.duration_secs)}</td>
          <td>{s.project || '-'}</td>
          <td class="num">{s.api_call_count ?? '-'}</td>
          <td class="num">{fmtTokens(s.input_tokens)}</td>
          <td class="num">{fmtTokens(s.output_tokens)}</td>
          <td class="num">{fmtCost(s.cost_usd)}</td>
          <td class="num">{s.files_changed ?? '-'}</td>
          <td class="summary">{s.summary || '-'}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
