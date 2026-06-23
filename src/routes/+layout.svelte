<script lang="ts">
  import { page } from "$app/stores";
  import { darkMode } from "$lib/stores/theme.svelte";

  function toggleDark() {
    darkMode.update((v) => !v);
  }

  // 1.7px-stroke line icons (viewBox 0 0 24 24, currentColor) per design handoff ICONS.md
  const icons: Record<string, string> = {
    dashboard:
      '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
    transactions:
      '<rect x="2" y="5" width="20" height="14" rx="2.5"/><line x1="2" y1="10" x2="22" y2="10"/>',
    ask: '<rect x="3" y="4" width="18" height="12" rx="3"/><path d="M8 16v4l4-4"/>',
    categories:
      '<path d="M4 12V5a1 1 0 0 1 1-1h7l8 8-8 8z"/><circle cx="8.5" cy="8.5" r="1.4"/>',
    forecasting:
      '<polyline points="3 17 9 11 13 15 21 7"/><polyline points="15 7 21 7 21 13"/>',
    goals:
      '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="4"/><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none"/>',
    recurring:
      '<path d="M4 9a7 7 0 0 1 12-3l2 2"/><path d="M20 15a7 7 0 0 1-12 3l-2-2"/><polyline points="18 4 18 8 14 8"/><polyline points="6 20 6 16 10 16"/>',
    settings:
      '<line x1="4" y1="8" x2="20" y2="8"/><circle cx="9" cy="8" r="2.3"/><line x1="4" y1="16" x2="20" y2="16"/><circle cx="15" cy="16" r="2.3"/>',
  };
  const house =
    '<path d="M4 11l8-7 8 7"/><path d="M6 10v9h12v-9"/>';
  const sun =
    '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>';
  const moon =
    '<path d="M20 14.5A8 8 0 1 1 9.5 4 6.5 6.5 0 0 0 20 14.5z"/>';

  const navItems = [
    { href: "/", label: "Dashboard", icon: "dashboard", exact: true },
    { href: "/transactions", label: "Transactions", icon: "transactions" },
    { href: "/ask", label: "Ask", icon: "ask" },
    { href: "/categories", label: "Categories", icon: "categories" },
    { href: "/forecasting", label: "Forecasting", icon: "forecasting" },
    { href: "/goals", label: "Goals", icon: "goals" },
    { href: "/recurring", label: "Recurring", icon: "recurring" },
    { href: "/settings", label: "Settings", icon: "settings" },
  ];

  function isActive(href: string, exact: boolean | undefined): boolean {
    const path = $page.url.pathname;
    return exact ? path === href : path.startsWith(href);
  }
</script>

<div class="app-layout" class:dark={$darkMode}>
  <nav class="sidebar">
    <div class="brand">
      <span class="brand-mark">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{@html house}</svg>
      </span>
      <span class="brand-name">Family Finance</span>
      <button class="theme-toggle" onclick={toggleDark} aria-label="Toggle dark mode">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{@html $darkMode ? sun : moon}</svg>
      </button>
    </div>

    <ul class="nav-list">
      {#each navItems as item}
        <li>
          <a href={item.href} class="nav-item" class:active={isActive(item.href, item.exact)}>
            <span class="nav-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{@html icons[item.icon]}</svg>
            </span>
            {item.label}
          </a>
        </li>
      {/each}
    </ul>

    <div class="household">
      <span class="household-avatar">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{@html house}</svg>
      </span>
      <div class="household-text">
        <span class="household-name">Our Home</span>
        <span class="household-sub">Household</span>
      </div>
    </div>
  </nav>

  <main class="main-content">
    <slot />
  </main>
</div>

<style>
  :global(*) {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  :global(body) {
    font-family: "Figtree", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    overflow: hidden;
  }

  /* Bitter (serif) for page titles, section headers and emphasised numbers */
  :global(h1),
  :global(h2),
  :global(h3) {
    font-family: "Bitter", Georgia, serif;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  :global(*) {
    /* Hearth — light */
    --bg-primary: #ffffff;
    --bg-secondary: #fbf4e9;
    --bg-card: #ffffff;
    --text-primary: #33302a;
    --text-secondary: #7b7468;
    --text-muted: #a89f90;
    --border-color: #ece0cc;
    --sidebar-bg: #ffffff;
    --main-bg: #f2eadc;

    --accent: #7f9a6f;
    --accent-soft: #e7eedf;
    --nav-active-fg: #4f6b45;
    --pos: #6f9466;
    --neg: #c77a5a;
    --amber: #c99a52;
    --track: #efe6d6;
    --app-shadow: 0 4px 16px rgba(80, 70, 50, 0.06);
    --radius-card: 18px;
    --radius-pill: 999px;
    --c1: #7f9a6f;
    --c2: #e2a765;
    --c3: #c98f5e;
    --c4: #9db58c;
    --c5: #d98c6a;
    --c6: #e8c79b;
  }

  :global(.dark) {
    /* Hearth — dark */
    --bg-primary: #29241e;
    --bg-secondary: #322b23;
    --bg-card: #29241e;
    --text-primary: #f3ecde;
    --text-secondary: #b5ab99;
    --text-muted: #857c6c;
    --border-color: #3b342b;
    --sidebar-bg: #241f19;
    --main-bg: #1e1a15;

    --accent: #9dba8b;
    --accent-soft: rgba(157, 186, 139, 0.15);
    --nav-active-fg: #b9d2a7;
    --pos: #9dba8b;
    --neg: #db9c7c;
    --amber: #e2a765;
    --track: #3b342b;
    --app-shadow: 0 4px 18px rgba(0, 0, 0, 0.28);
    --c1: #9dba8b;
    --c2: #e2a765;
    --c3: #d49a6a;
    --c4: #aac39b;
    --c5: #e0987a;
    --c6: #e8c79b;
  }

  .app-layout {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 238px;
    min-width: 238px;
    background: var(--sidebar-bg);
    border-right: 1px solid var(--border-color);
    color: var(--text-secondary);
    display: flex;
    flex-direction: column;
    padding: 22px 16px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 0 6px 18px;
  }
  .brand-mark {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .brand-mark svg {
    width: 18px;
    height: 18px;
  }
  .brand-name {
    font-family: "Bitter", Georgia, serif;
    font-size: 17px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }
  .theme-toggle {
    margin-left: auto;
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .theme-toggle svg {
    width: 16px;
    height: 16px;
  }
  .theme-toggle:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .nav-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 14px;
    border-radius: var(--radius-pill);
    color: var(--text-secondary);
    font-size: 13.5px;
    font-weight: 500;
    text-decoration: none;
    transition: background 0.15s, color 0.15s;
  }
  .nav-item:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }
  .nav-item.active {
    background: var(--accent-soft);
    color: var(--nav-active-fg);
    font-weight: 600;
  }
  .nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .nav-icon svg {
    width: 17px;
    height: 17px;
  }

  .household {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px;
    border-radius: 16px;
    background: var(--accent-soft);
  }
  .household-avatar {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .household-avatar svg {
    width: 18px;
    height: 18px;
  }
  .household-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .household-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .household-sub {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .main-content {
    flex: 1;
    background: var(--main-bg);
    overflow-y: auto;
    padding: 2rem 2.5rem;
  }
</style>
