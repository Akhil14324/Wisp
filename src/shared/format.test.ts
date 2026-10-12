import { describe, expect, it } from "vitest";
import { chordLabel, describeWhen, duration, filterTodos, isoToLocalInput, localInputToIso, mmss, truncate, whenLabel } from "./format";
import type { Todo } from "./types";

const todo = (id: number, due: string | null, done = false): Todo => ({
  id,
  title: `t${id}`,
  notes: null,
  due_at: due,
  done,
  sort_order: id,
  created_at: "2026-10-01T00:00:00Z",
  completed_at: null,
  snoozed_until: null,
});

describe("format", () => {
  it("round-trips datetime-local values", () => {
    const iso = localInputToIso("2026-10-12T18:00");
    expect(iso).toMatch(/Z$/);
    expect(isoToLocalInput(iso)).toBe("2026-10-12T18:00");
    expect(localInputToIso("")).toBeNull();
  });

  it("labels times relative to today", () => {
    const now = new Date(2026, 9, 12, 9, 0);
    expect(whenLabel(new Date(2026, 9, 12, 18, 0).toISOString(), now)).toBe("18:00");
    expect(whenLabel(new Date(2026, 9, 13, 9, 5).toISOString(), now)).toBe("Tomorrow 09:05");
    expect(whenLabel(null, now)).toBe("");
  });

  it("describes schedules", () => {
    expect(describeWhen({ kind: "daily", time: "09:00" })).toBe("Every day at 09:00");
    expect(describeWhen({ kind: "weekly", time: "18:00", days: ["mon", "wed"] })).toBe("Every Mon, Wed at 18:00");
    expect(describeWhen({ kind: "once", date: "2026-11-10", time: "11:00" })).toBe("Once, 2026-11-10 at 11:00");
  });

  it("filters to-dos", () => {
    const now = new Date(2026, 9, 12, 9, 0);
    const list = [
      todo(1, null),
      todo(2, new Date(2026, 9, 12, 18, 0).toISOString()),
      todo(3, new Date(2026, 9, 14, 9, 0).toISOString()),
      todo(4, null, true),
      todo(5, new Date(2026, 9, 10, 9, 0).toISOString()),
    ];
    expect(filterTodos(list, "today", now).map((t) => t.id)).toEqual([1, 2, 5]);
    expect(filterTodos(list, "upcoming", now).map((t) => t.id)).toEqual([3]);
    expect(filterTodos(list, "done", now).map((t) => t.id)).toEqual([4]);
  });

  it("formats small things", () => {
    expect(duration(450)).toBe("450 ms");
    expect(duration(2500)).toBe("2.5 s");
    expect(mmss(125000)).toBe("2:05");
    expect(truncate("hello world", 6)).toBe("hello…");
    expect(chordLabel("ctrl+win")).toBe("Ctrl + Win");
    expect(chordLabel("esc*3")).toBe("Esc ×3");
  });
});
