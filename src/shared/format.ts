// Pure formatting helpers shared by the windows.
import type { Todo, When } from "./types";

const pad = (n: number) => String(n).padStart(2, "0");

/** "2026-10-12T18:00" for <input type="datetime-local"> from a UTC ISO string. */
export function isoToLocalInput(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** UTC ISO string from a datetime-local value, or null when empty. */
export function localInputToIso(v: string): string | null {
  if (!v) return null;
  const d = new Date(v);
  return Number.isNaN(d.getTime()) ? null : d.toISOString().replace(/\.\d{3}Z$/, "Z");
}

export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** Short time label: "18:00", "Tomorrow 09:00", "Mon 12 Oct 09:00". */
export function whenLabel(iso: string | null, now: Date = new Date()): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (sameDay(d, now)) return hm;
  const tomorrow = new Date(now);
  tomorrow.setDate(now.getDate() + 1);
  if (sameDay(d, tomorrow)) return `Tomorrow ${hm}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (sameDay(d, yesterday)) return `Yesterday ${hm}`;
  return `${d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })} ${hm}`;
}

/** "3 min ago", "2 h ago", or a date. */
export function ago(iso: string, now: Date = new Date()): string {
  const d = new Date(iso);
  const s = Math.round((now.getTime() - d.getTime()) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return whenLabel(iso, now);
}

export function duration(ms: number | null): string {
  if (ms === null || ms === undefined) return "";
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;
}

export function mmss(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${pad(s % 60)}`;
}

const DAY_NAMES: Record<string, string> = { mon: "Mon", tue: "Tue", wed: "Wed", thu: "Thu", fri: "Fri", sat: "Sat", sun: "Sun" };

export function describeWhen(w: When): string {
  const t = w.time ?? "";
  switch (w.kind) {
    case "once":
      return `Once, ${w.date ?? ""} at ${t}`;
    case "daily":
      return `Every day at ${t}`;
    case "weekdays":
      return `Weekdays at ${t}`;
    case "weekly":
      return `Every ${(w.days ?? []).map((d) => DAY_NAMES[d] ?? d).join(", ")} at ${t}`;
    case "monthly":
      return `Monthly on day ${w.day_of_month ?? 1} at ${t}`;
    case "cron":
      return `Cron: ${w.cron ?? ""}`;
  }
}

export type TodoFilter = "today" | "upcoming" | "done";

/** FR-TODO-04 filters. Today = open items due today, overdue, or undated. */
export function filterTodos(todos: Todo[], filter: TodoFilter, now: Date = new Date()): Todo[] {
  const endOfToday = new Date(now);
  endOfToday.setHours(23, 59, 59, 999);
  return todos.filter((t) => {
    if (filter === "done") return t.done;
    if (t.done) return false;
    const due = t.due_at ? new Date(t.due_at) : null;
    if (filter === "today") return !due || due <= endOfToday;
    return !!due && due > endOfToday;
  });
}

export function truncate(s: string, n: number): string {
  return s.length <= n ? s : `${s.slice(0, n - 1)}…`;
}

/** Pretty chord label: "ctrl+win" -> "Ctrl + Win". */
export function chordLabel(c: string): string {
  return c
    .split("+")
    .map((k) => (k === "esc*3" ? "Esc ×3" : k.charAt(0).toUpperCase() + k.slice(1)))
    .join(" + ");
}
