<script>
  import { onMount } from 'svelte';
  import Sessions from './pages/Sessions.svelte';
  import SessionDetail from './pages/SessionDetail.svelte';

  let route = $state({ page: 'sessions', id: null });

  function parseHash() {
    const hash = window.location.hash || '#/';
    if (hash.startsWith('#/session/')) {
      const id = hash.slice('#/session/'.length);
      route = { page: 'session', id };
    } else if (hash === '#/config') {
      route = { page: 'config', id: null };
    } else {
      route = { page: 'sessions', id: null };
    }
  }

  onMount(() => {
    parseHash();
    window.addEventListener('hashchange', parseHash);
    return () => window.removeEventListener('hashchange', parseHash);
  });
</script>

<div class="header">
  <h1><a href="#/" style="color: inherit; text-decoration: none;">LAUDEC</a></h1>
  <nav>
    <a href="#/">SESSIONS</a>
    <a href="#/config">CONFIG</a>
  </nav>
</div>

<div class="container">
  {#if route.page === 'sessions'}
    <Sessions />
  {:else if route.page === 'session' && route.id}
    <SessionDetail id={route.id} />
  {:else if route.page === 'config'}
    <div class="stub">Config viewer coming soon</div>
  {/if}
</div>
