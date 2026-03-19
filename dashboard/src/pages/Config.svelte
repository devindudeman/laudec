<script>
  import { onMount } from 'svelte';
  import { fetchConfig } from '../lib/api.js';

  let config = $state(null);
  let error = $state(null);

  const sections = [
    { key: 'proxy', label: 'Proxy' },
    { key: 'telemetry', label: 'Telemetry' },
    { key: 'dashboard', label: 'Dashboard' },
    { key: 'session', label: 'Session' },
    { key: 'claude', label: 'Claude' },
    { key: 'sandbox', label: 'Sandbox' },
    { key: 'permissions', label: 'Permissions' },
  ];

  onMount(async () => {
    try {
      config = await fetchConfig();
    } catch (e) {
      error = e.message;
    }
  });

  function formatValue(val) {
    if (val === null || val === undefined) return { text: '(not set)', cls: 'config-val-null' };
    if (typeof val === 'boolean') return { text: String(val), cls: val ? 'config-val-true' : 'config-val-false' };
    if (Array.isArray(val)) return { text: val.length ? val.join(', ') : '(empty)', cls: val.length ? '' : 'config-val-null' };
    if (typeof val === 'object') return { text: JSON.stringify(val), cls: '' };
    return { text: String(val), cls: '' };
  }
</script>

{#if error}
  <div class="stub">Error loading config: {error}</div>
{:else if !config}
  <div class="loading">Loading config...</div>
{:else}
  <div class="config-source">Loaded from: <strong>{config.source}</strong></div>

  {#each sections as { key, label }}
    {#if config[key]}
      <div class="config-section">
        <div class="section-title">{label}</div>
        <div class="config-grid">
          {#each Object.entries(config[key]) as [k, v]}
            {@const fv = formatValue(v)}
            <div class="config-key">{k}</div>
            <div class="config-val {fv.cls}">{fv.text}</div>
          {/each}
        </div>
      </div>
    {/if}
  {/each}
{/if}
