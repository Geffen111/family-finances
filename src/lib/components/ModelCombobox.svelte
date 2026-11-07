<script module lang="ts">
  // The catalogue row the picker renders, mirroring
  // `services::openrouter::ModelInfo`. Prices are USD per million tokens.
  export interface ModelInfo {
    id: string;
    name: string;
    context_length: number | null;
    prompt_price: number | null;
    completion_price: number | null;
  }
</script>

<script lang="ts">
  // Searchable picker over OpenRouter's catalogue (hundreds of models, so a
  // native <select> is unusable). Same shape as CategoryCombobox: a button
  // that opens a fixed-position, searchable menu. A model id typed into the
  // search box can be chosen as-is, so an unlisted model — or one added after
  // this build — is still reachable, and the picker keeps working if the
  // catalogue fetch failed.
  let {
    models,
    value,
    onSelect,
    allowNone = false,
    noneLabel = "None",
    placeholder = "Choose a model…",
    disabled = false,
  }: {
    models: ModelInfo[];
    value: string;
    onSelect: (id: string) => void;
    allowNone?: boolean;
    noneLabel?: string;
    placeholder?: string;
    disabled?: boolean;
  } = $props();

  let open = $state(false);
  let query = $state("");
  let highlightIdx = $state(0);
  let menuStyle = $state("");
  let btnEl: HTMLButtonElement;
  let inputEl = $state<HTMLInputElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);

  function priceLabel(m: ModelInfo): string {
    if (m.prompt_price == null && m.completion_price == null) return "";
    const fmt = (p: number | null) =>
      p == null ? "?" : p === 0 ? "free" : `$${p < 1 ? p.toFixed(3) : p.toFixed(2)}`;
    return `${fmt(m.prompt_price)} in / ${fmt(m.completion_price)} out per 1M tokens`;
  }

  let selected = $derived(models.find((m) => m.id === value));
  let currentLabel = $derived(
    !value ? (allowNone ? noneLabel : placeholder) : (selected?.name ?? value),
  );

  type Item = { id: string; label: string; sub: string };
  let items = $derived.by<Item[]>(() => {
    const q = query.trim().toLowerCase();
    const out: Item[] = [];
    if (allowNone && (!q || noneLabel.toLowerCase().includes(q))) {
      out.push({ id: "", label: noneLabel, sub: "" });
    }
    // A typed id that isn't in the catalogue is still offerable.
    if (q && !models.some((m) => m.id.toLowerCase() === q)) {
      out.push({ id: query.trim(), label: `Use "${query.trim()}"`, sub: "custom model id" });
    }
    for (const m of models) {
      if (q && !m.id.toLowerCase().includes(q) && !m.name.toLowerCase().includes(q)) continue;
      out.push({ id: m.id, label: m.name, sub: [m.id, priceLabel(m)].filter(Boolean).join(" · ") });
    }
    return out;
  });

  function openMenu() {
    if (disabled) return;
    const r = btnEl.getBoundingClientRect();
    const w = Math.max(r.width, 320);
    const spaceBelow = window.innerHeight - r.bottom;
    const openUp = spaceBelow < 320 && r.top > spaceBelow;
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

  function choose(id: string) {
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

  $effect(() => {
    if (!open) return;
    highlightIdx;
    queueMicrotask(() =>
      menuEl?.querySelector(".cb-active")?.scrollIntoView({ block: "nearest" }),
    );
  });

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
  <span class="cb-label" class:cb-placeholder={!value && !allowNone}>{currentLabel}</span>
  <span class="cb-caret">▾</span>
</button>
{#if value && selected}
  <p class="cb-meta">
    {selected.id}{priceLabel(selected) ? ` · ${priceLabel(selected)}` : ""}
  </p>
{:else if value}
  <p class="cb-meta">{value} · not in the catalogue</p>
{/if}

{#if open}
  <div class="cb-backdrop" role="presentation" onclick={closeMenu}></div>
  <div class="cb-menu" bind:this={menuEl} style={menuStyle} role="listbox" tabindex="-1">
    <input
      bind:this={inputEl}
      class="cb-search"
      type="text"
      placeholder="Search models, or type a model id…"
      bind:value={query}
      onkeydown={onKey}
      oninput={() => (highlightIdx = 0)}
    />
    <div class="cb-options">
      {#each items as item, i (item.id || "none")}
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
          <span class="cb-option-label">{item.label}</span>
          {#if item.sub}<span class="cb-option-sub">{item.sub}</span>{/if}
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
    padding: 0.625rem 0.75rem;
    font-size: 0.95rem;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    background: var(--bg-card);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    box-sizing: border-box;
  }
  .cb-trigger:hover { border-color: var(--text-muted); }
  .cb-trigger:disabled { opacity: 0.5; cursor: not-allowed; }
  .cb-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cb-placeholder { color: var(--text-muted); }
  .cb-caret { color: var(--text-muted); font-size: 0.7rem; flex-shrink: 0; }
  .cb-meta {
    margin: 0.35rem 0 0;
    font-size: 0.75rem;
    color: var(--text-secondary);
    word-break: break-all;
  }

  .cb-backdrop { position: fixed; inset: 0; z-index: 500; background: transparent; }
  .cb-menu {
    position: fixed;
    z-index: 501;
    display: flex;
    flex-direction: column;
    max-height: 340px;
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
  .cb-search:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .cb-options { overflow-y: auto; padding: 0 0.3rem 0.3rem; }
  .cb-option {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    width: 100%;
    text-align: left;
    padding: 0.35rem 0.5rem;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    overflow: hidden;
  }
  .cb-option-label {
    font-size: 0.82rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb-option-sub {
    font-size: 0.7rem;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb-active { background: var(--bg-secondary); }
  .cb-selected .cb-option-label { color: var(--accent); font-weight: 600; }
  .cb-empty { padding: 0.5rem; font-size: 0.8rem; color: var(--text-muted); text-align: center; }
</style>
