<script lang="ts">
  // A searchable category picker used per transaction row. Unlike a native
  // <select> (which renders all ~89 <option>s for every row and froze large
  // accounts), a closed combobox is just a button — the option list only
  // exists while one row's menu is open. The menu is position:fixed and
  // anchored to the trigger so the scrolling table doesn't clip it.
  interface Cat {
    id: number;
    path: string;
  }

  let {
    categories,
    value,
    onSelect,
    disabled = false,
  }: {
    categories: Cat[];
    value: number | null;
    onSelect: (id: number | null) => void;
    disabled?: boolean;
  } = $props();

  let open = $state(false);
  let query = $state("");
  let highlightIdx = $state(0);
  let menuStyle = $state("");
  let btnEl: HTMLButtonElement;
  let inputEl = $state<HTMLInputElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);

  let currentLabel = $derived(
    value == null
      ? "Uncategorised"
      : (categories.find((c) => c.id === value)?.path ?? "Uncategorised"),
  );

  // "Uncategorised" sits at the top (to clear), shown unless the query clearly
  // doesn't match it; the rest is a case-insensitive path filter.
  type Item = { id: number | null; label: string };
  let items = $derived.by<Item[]>(() => {
    const q = query.trim().toLowerCase();
    const cats = q ? categories.filter((c) => c.path.toLowerCase().includes(q)) : categories;
    const out: Item[] = [];
    if (!q || "uncategorised".includes(q)) out.push({ id: null, label: "Uncategorised" });
    for (const c of cats) out.push({ id: c.id, label: c.path });
    return out;
  });

  function openMenu() {
    if (disabled) return;
    const r = btnEl.getBoundingClientRect();
    const w = Math.max(r.width, 220);
    const spaceBelow = window.innerHeight - r.bottom;
    const openUp = spaceBelow < 280 && r.top > spaceBelow;
    const vert = openUp
      ? `bottom:${Math.round(window.innerHeight - r.top + 2)}px`
      : `top:${Math.round(r.bottom + 2)}px`;
    menuStyle = `left:${Math.round(r.left)}px; ${vert}; width:${Math.round(w)}px;`;
    query = "";
    highlightIdx = 0;
    open = true;
    queueMicrotask(() => inputEl?.focus());
  }

  function closeMenu() {
    open = false;
  }

  function choose(id: number | null) {
    if (id !== value) onSelect(id);
    closeMenu();
    btnEl.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeMenu();
      btnEl.focus();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      highlightIdx = Math.min(highlightIdx + 1, items.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      highlightIdx = Math.max(highlightIdx - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items[highlightIdx];
      if (item) choose(item.id);
    }
  }

  // Keep the highlighted option in view during keyboard navigation.
  $effect(() => {
    if (!open) return;
    highlightIdx;
    queueMicrotask(() => menuEl?.querySelector(".cb-active")?.scrollIntoView({ block: "nearest" }));
  });

  // Any scroll (capture phase catches the inner table) or resize closes the
  // menu — but not when the scroll originates inside the menu itself (e.g.
  // scrolling the category list via trackpad).
  $effect(() => {
    if (!open) return;
    const onScroll = (e: Event) => {
      if (menuEl && e.target instanceof Node && menuEl.contains(e.target)) return;
      closeMenu();
    };
    const onResize = () => closeMenu();
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

<button
  bind:this={btnEl}
  type="button"
  class="cb-trigger"
  {disabled}
  aria-haspopup="listbox"
  aria-expanded={open}
  onclick={() => (open ? closeMenu() : openMenu())}
>
  <span class="cb-label" class:cb-placeholder={value == null}>{currentLabel}</span>
  <span class="cb-caret">▾</span>
</button>

{#if open}
  <div class="cb-backdrop" role="presentation" onclick={closeMenu}></div>
  <div class="cb-menu" bind:this={menuEl} style={menuStyle} role="listbox" tabindex="-1">
    <input
      bind:this={inputEl}
      class="cb-search"
      type="text"
      placeholder="Search categories…"
      bind:value={query}
      onkeydown={onKey}
      oninput={() => (highlightIdx = 0)}
    />
    <div class="cb-options">
      {#each items as item, i (item.id ?? "none")}
        <button
          type="button"
          class="cb-option"
          class:cb-active={i === highlightIdx}
          class:cb-selected={item.id === value}
          role="option"
          aria-selected={item.id === value}
          onmouseenter={() => (highlightIdx = i)}
          onclick={() => choose(item.id)}
        >
          {item.label}
        </button>
      {/each}
      {#if items.length === 0}
        <div class="cb-empty">No matches</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .cb-trigger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.3rem;
    width: 100%;
    padding: 0.3rem 0.4rem;
    font-size: 0.8rem;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    background: var(--bg-card);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
  }
  .cb-trigger:hover { border-color: var(--text-muted); }
  .cb-trigger:disabled { opacity: 0.5; cursor: not-allowed; }
  .cb-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cb-placeholder { color: var(--text-muted); }
  .cb-caret { color: var(--text-muted); font-size: 0.7rem; flex-shrink: 0; }

  .cb-backdrop { position: fixed; inset: 0; z-index: 500; background: transparent; }
  .cb-menu {
    position: fixed;
    z-index: 501;
    display: flex;
    flex-direction: column;
    max-height: 300px;
    overflow: hidden;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18);
  }
  .cb-search {
    margin: 0.4rem;
    padding: 0.4rem 0.5rem;
    font-size: 0.82rem;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--text-primary);
  }
  .cb-search:focus { outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .cb-options { overflow-y: auto; padding: 0 0.3rem 0.3rem; }
  .cb-option {
    display: block;
    width: 100%;
    text-align: left;
    padding: 0.35rem 0.5rem;
    font-size: 0.82rem;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb-active { background: var(--bg-secondary); }
  .cb-selected { color: var(--accent); font-weight: 600; }
  .cb-empty { padding: 0.5rem; font-size: 0.8rem; color: var(--text-muted); text-align: center; }
</style>
