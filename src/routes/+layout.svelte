<script lang="ts">
  import { page } from "$app/stores";
  import { darkMode } from "$lib/stores/theme.svelte";

  function toggleDark() {
    darkMode.update((v) => !v);
  }
</script>

<div class="app-layout" class:dark={$darkMode}>
  <nav class="sidebar">
    <div class="sidebar-header">
      <span>Family Finance</span>
      <button class="theme-toggle" onclick={toggleDark} aria-label="Toggle dark mode">
        {$darkMode ? "☀️" : "🌙"}
      </button>
    </div>
    <ul class="nav-list">
      <li>
        <a href="/" class="nav-item" class:active={$page.url.pathname === "/"}>
          <span class="nav-icon">📊</span> Dashboard
        </a>
      </li>
      <li>
        <a href="/transactions" class="nav-item" class:active={$page.url.pathname.startsWith("/transactions")}>
          <span class="nav-icon">💳</span> Transactions
        </a>
      </li>
      <li>
        <a href="/ask" class="nav-item" class:active={$page.url.pathname.startsWith("/ask")}>
          <span class="nav-icon">💬</span> Ask
        </a>
      </li>
      <li>
        <a href="/categories" class="nav-item" class:active={$page.url.pathname.startsWith("/categories")}>
          <span class="nav-icon">🏷️</span> Categories
        </a>
      </li>
      <li>
        <a href="/forecasting" class="nav-item" class:active={$page.url.pathname.startsWith("/forecasting")}>
          <span class="nav-icon">📈</span> Forecasting
        </a>
      </li>
      <li>
        <a href="/goals" class="nav-item" class:active={$page.url.pathname.startsWith("/goals")}>
          <span class="nav-icon">🎯</span> Goals
        </a>
      </li>
      <li>
        <a href="/recurring" class="nav-item" class:active={$page.url.pathname.startsWith("/recurring")}>
          <span class="nav-icon">🔁</span> Recurring
        </a>
      </li>
      <li>
        <a href="/settings" class="nav-item" class:active={$page.url.pathname.startsWith("/settings")}>
          <span class="nav-icon">⚙️</span> Settings
        </a>
      </li>
    </ul>
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
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    overflow: hidden;
  }

  :global(*) {
    --bg-primary: #ffffff;
    --bg-secondary: #f9fafb;
    --bg-card: #ffffff;
    --text-primary: #111827;
    --text-secondary: #6b7280;
    --text-muted: #9ca3af;
    --border-color: #e5e7eb;
    --sidebar-bg: #111827;
    --main-bg: #f9fafb;
  }

  :global(.dark) {
    --bg-primary: #0f172a;
    --bg-secondary: #1e293b;
    --bg-card: #1e293b;
    --text-primary: #f1f5f9;
    --text-secondary: #94a3b8;
    --text-muted: #64748b;
    --border-color: #334155;
    --sidebar-bg: #020617;
    --main-bg: #0f172a;
  }

  .app-layout {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 240px;
    min-width: 240px;
    background: var(--sidebar-bg);
    color: #d1d5db;
    display: flex;
    flex-direction: column;
    padding: 1.5rem 0;
  }

  .sidebar-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 1.25rem;
    font-weight: 700;
    color: #fff;
    padding: 0 1.5rem 1.5rem;
    border-bottom: 1px solid #1f2937;
    margin-bottom: 0.5rem;
  }

  .theme-toggle {
    background: none;
    border: none;
    font-size: 1.1rem;
    cursor: pointer;
    padding: 0.25rem;
    line-height: 1;
    border-radius: 4px;
    transition: background 0.15s;
  }
  .theme-toggle:hover {
    background: rgba(255, 255, 255, 0.1);
  }

  .nav-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.75rem 1.5rem;
    color: #d1d5db;
    font-size: 0.95rem;
    text-decoration: none;
    transition: background 0.15s, color 0.15s;
  }

  .nav-item:hover {
    background: #1f2937;
    color: #fff;
  }

  .nav-item.active {
    background: #1e3a5f;
    color: #60a5fa;
  }

  .nav-icon {
    font-size: 1.1rem;
    width: 1.5rem;
    text-align: center;
  }

  .main-content {
    flex: 1;
    background: var(--main-bg);
    overflow-y: auto;
    padding: 2rem;
  }
</style>