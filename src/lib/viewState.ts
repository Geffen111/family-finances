// Per-page view state that survives navigating away and closing the app.
//
// Only *view* state belongs here -- the filters, date presets, sort order,
// chart selections and expanded rows that make a page "yours". Loaded data
// stays out (it is refetched, and the DB is the source of truth), as do modal
// open/close flags and in-flight spinners, which would otherwise restore the
// app mid-dialog.
//
// localStorage, not the DB: this is per-machine UI state, and the DB lives in
// OneDrive where it syncs across devices -- a filter you left on the laptop
// shouldn't reach into the desktop's view.

const PREFIX = "ff:view:";

// JSON can't carry a Set, and several pages keep multi-selections (trend
// categories, expanded rows) as one. Tag them on the way out and rebuild them
// on the way in, so page code keeps using plain Sets.
type SetEnvelope = { __set: unknown[] };

function isSetEnvelope(v: unknown): v is SetEnvelope {
  return typeof v === "object" && v !== null && Array.isArray((v as SetEnvelope).__set);
}

function replacer(_key: string, value: unknown): unknown {
  // JSON.stringify calls toJSON before the replacer sees a value, but Set has
  // no toJSON, so a Set still arrives here intact.
  return value instanceof Set ? { __set: [...value] } : value;
}

function reviver(_key: string, value: unknown): unknown {
  return isSetEnvelope(value) ? new Set(value.__set) : value;
}

/**
 * Read a page's saved view state, merged over `fallback`.
 *
 * The merge is one level deep and keyed off `fallback`, which makes the stored
 * blob forgiving: a key added to a page later picks up its default instead of
 * coming back undefined, and a key removed from the page is ignored rather
 * than resurrected. A value whose type no longer matches the default is
 * dropped too -- the usual cause is a page changing shape between versions.
 */
export function loadView<T extends Record<string, unknown>>(page: string, fallback: T): T {
  const out = { ...fallback };
  let stored: unknown;
  try {
    const raw = localStorage.getItem(PREFIX + page);
    if (!raw) return out;
    stored = JSON.parse(raw, reviver);
  } catch {
    // Unreadable (private mode, cleared site data, a half-written value) --
    // the defaults are a perfectly good answer.
    return out;
  }
  if (typeof stored !== "object" || stored === null) return out;

  for (const key of Object.keys(fallback) as (keyof T)[]) {
    const value = (stored as T)[key];
    if (value === undefined) continue;
    const def = fallback[key];
    if (def instanceof Set) {
      if (value instanceof Set) out[key] = value;
      continue;
    }
    // null is a legitimate stored value (e.g. "no tag filter"), and a default
    // of null tells us nothing about the expected type, so accept it as-is.
    if (def === null || value === null || typeof def === typeof value) {
      out[key] = value as T[keyof T];
    }
  }
  return out;
}

/**
 * Write a page's view state. Call it from a `$effect` that reads each value,
 * so Svelte re-runs it whenever one of them changes:
 *
 *     $effect(() => saveView("dashboard", { activePreset, customStart }));
 */
export function saveView(page: string, value: Record<string, unknown>): void {
  try {
    localStorage.setItem(PREFIX + page, JSON.stringify(value, replacer));
  } catch {
    // Storage full or blocked. Losing the view state is not worth an error in
    // the user's face, and the page works fine without it.
  }
}

/** Forget one page's view state (used by the per-page "Reset view" controls). */
export function clearView(page: string): void {
  try {
    localStorage.removeItem(PREFIX + page);
  } catch {
    /* nothing to do */
  }
}
