<script lang="ts">
  import { page } from "$app/stores";
  import { darkMode } from "$lib/stores/theme.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";

  let householdName = $state("Our Home");
  $effect(() => {
    invoke<string | null>("get_household_name").then((name) => {
      if (name) householdName = name;
    });
  });

  // --- Update check -------------------------------------------------------
  // The CI publishes build-info.json (the commit it was built from) to the
  // rolling "latest" GitHub release. We compare it to the commit baked into
  // this build; if they differ, a newer build is available. Downloads happen
  // by opening the release page in the browser (pick the right installer).
  //
  // The marker is fetched in Rust (latest_build_info) because the release-asset
  // host (release-assets.githubusercontent.com) sends no CORS header, so a
  // fetch() from the webview is silently blocked.
  const OWNER = "Geffen111";
  const REPO_NAME = "family-finances";
  const RELEASES_URL = `https://github.com/${OWNER}/${REPO_NAME}/releases/latest`;

  let updateInfo = $state<{ commit: string; builtAt?: string } | null>(null);

  async function checkForUpdate() {
    // Unstamped local/dev builds can't meaningfully compare — skip silently.
    if (typeof __APP_COMMIT__ === "undefined" || __APP_COMMIT__ === "dev") return;
    try {
      const info = await invoke<{ commit: string; builtAt?: string } | null>(
        "latest_build_info",
        { owner: OWNER, repo: REPO_NAME },
      );
      if (info?.commit && info.commit !== __APP_COMMIT__) {
        updateInfo = info;
      }
    } catch {
      // Offline, no release yet, or no marker — no banner.
    }
  }

  function formatBuiltAt(ts: string | undefined): string {
    if (!ts) return "";
    const d = new Date(ts);
    return isNaN(d.getTime())
      ? ""
      : d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  async function openUpdate() {
    try {
      await openUrl(RELEASES_URL);
    } catch {
      /* ignore */
    }
  }

  onMount(checkForUpdate);

  function toggleDark() {
    darkMode.update((v) => !v);
  }

  // Collapsed (icons-only) sidebar, remembered across launches.
  let collapsed = $state(false);
  onMount(() => {
    collapsed = localStorage.getItem("sidebarCollapsed") === "1";
  });
  function toggleCollapse() {
    collapsed = !collapsed;
    localStorage.setItem("sidebarCollapsed", collapsed ? "1" : "0");
  }
  const chevron = '<polyline points="15 18 9 12 15 6" />';

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
  <nav class="sidebar" class:collapsed>
    <div class="brand">
      <span class="brand-mark">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{@html house}</svg>
      </span>
      <span class="brand-name">Family Finance</span>
      <button class="theme-toggle" onclick={toggleDark} aria-label="Toggle dark mode">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{@html $darkMode ? sun : moon}</svg>
      </button>
    </div>

    <button
      class="collapse-btn"
      onclick={toggleCollapse}
      aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{@html chevron}</svg>
      <span class="collapse-label">Collapse</span>
    </button>

    <ul class="nav-list">
      {#each navItems as item}
        <li>
          <a href={item.href} class="nav-item" class:active={isActive(item.href, item.exact)} title={item.label}>
            <span class="nav-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{@html icons[item.icon]}</svg>
            </span>
            <span class="nav-label">{item.label}</span>
          </a>
        </li>
      {/each}
    </ul>

    <div class="sidebar-footer">
      {#if updateInfo}
        <button class="update-banner" onclick={openUpdate} title="Open the download page">
          <span class="update-icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 3v12" /><polyline points="7 10 12 15 17 10" /><path d="M5 20h14" />
            </svg>
          </span>
          <div class="update-text">
            <span class="update-title">Update available</span>
            <span class="update-sub">
              {formatBuiltAt(updateInfo.builtAt) ? `New build (${formatBuiltAt(updateInfo.builtAt)}) — click to get it` : "Click to download"}
            </span>
          </div>
        </button>
      {/if}

      <div class="household">
        <span class="household-avatar">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{@html house}</svg>
        </span>
        <div class="household-text">
          <span class="household-name">{householdName}</span>
          <span class="household-sub">Household</span>
        </div>
      </div>
    </div>
  </nav>

  <main class="main-content">
    <slot />
  </main>
</div>

<Toast />

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

  /* Shared button system. Pages add their own one-off colour variants
     (e.g. .btn-ai, .btn-warning, .btn-suggest, .btn-toggle, .btn-edit). */
  :global(.btn) {
    padding: 0.5rem 1rem;
    border: 1px solid var(--border-color);
    border-radius: var(--radius-pill);
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s, filter 0.15s;
  }
  :global(.btn:hover) { background: var(--bg-secondary); }
  :global(.btn:disabled) { opacity: 0.5; cursor: not-allowed; }
  :global(.btn-sm) { padding: 0.3rem 0.65rem; font-size: 0.8rem; }
  :global(.btn-primary) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(.btn-primary:hover) { background: var(--accent); filter: brightness(0.95); }
  :global(.btn-import) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(.btn-import:hover) { background: var(--accent); filter: brightness(0.95); }
  :global(.btn-add) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(.btn-add:hover) { background: var(--accent); filter: brightness(0.95); }
  :global(.btn-danger), :global(.btn-delete) { background: var(--neg); color: #fff; border-color: var(--neg); }
  :global(.btn-danger:hover), :global(.btn-delete:hover) { background: var(--neg); filter: brightness(0.95); }

  :root {
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
    --neg-soft: rgba(199, 122, 90, 0.14);
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
    --neg-soft: rgba(219, 156, 124, 0.16);
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
    transition: width 0.18s ease, min-width 0.18s ease, padding 0.18s ease;
  }
  .sidebar.collapsed {
    width: 66px;
    min-width: 66px;
    padding: 22px 10px;
  }

  .collapse-btn {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    margin-bottom: 10px;
    padding: 8px 14px;
    border: none;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .collapse-btn:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }
  .collapse-btn svg {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    transition: transform 0.18s ease;
  }
  .collapsed .collapse-btn {
    justify-content: center;
    padding: 8px;
  }
  .collapsed .collapse-btn svg {
    transform: rotate(180deg);
  }

  /* Collapsed (icons-only) state: hide text, centre the icons. */
  .collapsed .brand {
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 0 0 18px;
  }
  .collapsed .brand-name,
  .collapsed .collapse-label,
  .collapsed .nav-label,
  .collapsed .household-text,
  .collapsed .update-text {
    display: none;
  }
  .collapsed .theme-toggle {
    margin-left: 0;
  }
  .collapsed .nav-item {
    justify-content: center;
    padding: 10px;
  }
  .collapsed .household,
  .collapsed .update-banner {
    justify-content: center;
    padding: 11px 8px;
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

  .sidebar-footer {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .update-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 11px 12px;
    border-radius: 16px;
    border: 1px solid var(--accent);
    background: var(--accent-soft);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: filter 0.15s, transform 0.1s;
    animation: update-in 0.25s ease-out;
  }
  .update-banner:hover {
    filter: brightness(0.97);
    transform: translateY(-1px);
  }
  @keyframes update-in {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .update-icon {
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .update-icon svg {
    width: 16px;
    height: 16px;
  }
  .update-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    min-width: 0;
  }
  .update-title {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--nav-active-fg);
  }
  .update-sub {
    font-size: 10.5px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .household {
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
