import { GripVertical, Pencil, Plus, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useReload } from "../../shared/events";
import { errorMessage, ipc } from "../../shared/ipc";
import { filterTodos, isoToLocalInput, localInputToIso, type TodoFilter, whenLabel } from "../../shared/format";
import type { Todo } from "../../shared/types";
import { Button, Empty, ErrorText, IconButton, Input } from "../../shared/ui";
import { PageTitle } from "../App";

const FILTERS: { id: TodoFilter; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "upcoming", label: "Upcoming" },
  { id: "done", label: "Done" },
];

/** To-do list (FR-TODO-04): filters, add, edit, tick, delete, drag to reorder. */
export function TodoPage() {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [filter, setFilter] = useState<TodoFilter>("today");
  const [title, setTitle] = useState("");
  const [due, setDue] = useState("");
  const [err, setErr] = useState("");
  const [editing, setEditing] = useState<number | null>(null);
  const dragId = useRef<number | null>(null);
  const load = () => ipc.todosList().then(setTodos).catch(() => {});
  useReload(["todos-changed"], load);

  const visible = filterTodos(todos, filter);

  const add = async () => {
    if (!title.trim()) return;
    try {
      await ipc.todosCreate(title.trim(), localInputToIso(due));
      setTitle("");
      setDue("");
      setErr("");
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  const onDrop = async (targetId: number) => {
    const from = dragId.current;
    dragId.current = null;
    if (from === null || from === targetId) return;
    const ids = todos.map((t) => t.id);
    const fi = ids.indexOf(from);
    const ti = ids.indexOf(targetId);
    ids.splice(fi, 1);
    ids.splice(ti, 0, from);
    setTodos(ids.map((id) => todos.find((t) => t.id === id)!));
    await ipc.todosReorder(ids).catch(() => load());
  };

  return (
    <>
      <PageTitle title="To-do" sub='Say "add call Mom to my to-do list" or "add gym at 6 PM" in command mode.' />
      <div className="flex gap-2 mb-4">
        <Input aria-label="New to-do" className="flex-1" placeholder="Add a to-do" value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
        <Input aria-label="Due date and time" type="datetime-local" value={due} onChange={(e) => setDue(e.target.value)} />
        <Button variant="primary" onClick={add} disabled={!title.trim()}>
          <Plus size={14} /> Add
        </Button>
      </div>
      <ErrorText>{err}</ErrorText>
      <div role="tablist" aria-label="Filter" className="flex gap-1 mb-3">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            role="tab"
            type="button"
            aria-selected={filter === f.id}
            onClick={() => setFilter(f.id)}
            className={`h-8 px-3 rounded-[var(--radius)] text-[13px] ${filter === f.id ? "bg-[var(--panel-2)] font-medium" : "text-[var(--muted)] hover:text-[var(--fg)]"}`}
          >
            {f.label} <span className="text-[var(--muted)]">{filterTodos(todos, f.id).length}</span>
          </button>
        ))}
      </div>
      <div className="rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)]">
        {visible.length === 0 && <Empty>{filter === "done" ? "Nothing completed yet." : "All clear."}</Empty>}
        {visible.map((t) =>
          editing === t.id ? (
            <EditRow key={t.id} todo={t} onDone={() => setEditing(null)} />
          ) : (
            <div
              key={t.id}
              draggable={filter !== "done"}
              onDragStart={() => (dragId.current = t.id)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => onDrop(t.id)}
              className="group flex items-center gap-2.5 px-3 py-2 border-b border-[var(--border)] last:border-b-0"
            >
              {filter !== "done" && <GripVertical size={14} className="text-[var(--muted)] cursor-grab opacity-0 group-hover:opacity-100" aria-hidden />}
              <input
                type="checkbox"
                aria-label={`Mark ${t.title} ${t.done ? "not done" : "done"}`}
                className="h-4 w-4 accent-[var(--accent)]"
                checked={t.done}
                onChange={() => ipc.todosUpdate({ id: t.id, done: !t.done })}
              />
              <span className={`flex-1 min-w-0 truncate ${t.done ? "line-through text-[var(--muted)]" : ""}`}>{t.title}</span>
              {t.due_at && <span className={`text-[12px] ${!t.done && new Date(t.due_at) < new Date() ? "text-[var(--danger)]" : "text-[var(--muted)]"}`}>{whenLabel(t.due_at)}</span>}
              <IconButton label={`Edit ${t.title}`} onClick={() => setEditing(t.id)}>
                <Pencil size={14} />
              </IconButton>
              <IconButton label={`Delete ${t.title}`} onClick={() => ipc.todosDelete(t.id)}>
                <Trash2 size={14} />
              </IconButton>
            </div>
          ),
        )}
      </div>
      <p className="text-[12px] text-[var(--muted)] mt-3">Completed items are removed after 30 days.</p>
    </>
  );
}

function EditRow({ todo, onDone }: { todo: Todo; onDone: () => void }) {
  const [title, setTitle] = useState(todo.title);
  const [due, setDue] = useState(isoToLocalInput(todo.due_at));
  const [err, setErr] = useState("");
  const save = async () => {
    try {
      await ipc.todosUpdate({ id: todo.id, title: title.trim(), due_at: localInputToIso(due) });
      onDone();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  return (
    <div className="px-3 py-2 border-b border-[var(--border)] last:border-b-0">
      <div className="flex gap-2">
        <Input aria-label="Title" className="flex-1" value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && save()} autoFocus />
        <Input aria-label="Due" type="datetime-local" value={due} onChange={(e) => setDue(e.target.value)} />
        <Button variant="primary" onClick={save}>
          Save
        </Button>
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
      </div>
      <ErrorText>{err}</ErrorText>
    </div>
  );
}
