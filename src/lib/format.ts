import { format } from "date-fns";

// Shared date formatting so dates read consistently across the app
// (e.g. "5 Jul 2026") instead of raw "2026-07-05" ISO strings. Accepts a
// "YYYY-MM-DD" date or a fuller timestamp; returns "" for null/empty and
// falls back to the original string if it can't be parsed.
export function formatDate(value: string | null | undefined): string {
  if (!value) return "";
  const iso = value.length <= 10 ? `${value}T00:00:00` : value.replace(" ", "T");
  const d = new Date(iso);
  return isNaN(d.getTime()) ? value : format(d, "d MMM yyyy");
}
