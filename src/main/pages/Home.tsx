import { Bot, KeyRound, Play } from "lucide-react";
import { useState } from "react";
import { useReload } from "../../shared/events";
import { errorMessage, ipc } from "../../shared/ipc";
import { ago, filterTodos, truncate, whenLabel } from "../../shared/format";
import type { CommandEntry, Job, Todo } from "../../shared/types";
import { Badge, Button, Empty, Input, Section } from "../../shared/ui";
import { PageTitle } from "../App";
import { useMain } from "../store";

/** Home (section 8.2): today's to-dos, next jobs, last 5 commands, Run task box, key status. */
export function HomePage() {
  const { status, setPage } = useMain();
  const [todos, setTodos] = useState<Todo[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [history, setHistory] = useState<CommandEntry[]>([]);
  useReload(["todos-changed", "jobs-changed", "history-changed"], () => {
    ipc.todosList().then(setTodos).catch(() => {});
    ipc.jobsList().then(setJobs).catch(() => {});
    ipc.historyCommands().then((h) => setHistory(h.slice(0, 5))).catch(() => {});
  });
  const today = filterTodos(todos, "today").slice(0, 6);
  const next = jobs.filter((j) => j.enabled && j.next_run_at).sort((a, b) => (a.next_run_at! < b.next_run_at! ? -1 : 1)).slice(0, 4);
  const greeting = (() => {
    const h = new Date().getHours();
    return h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
  })();

  return (
    <>
      <PageTitle title={greeting} sub="Hold Ctrl + Win to dictate, Ctrl + Shift to circle and ask, Ctrl + Alt + Space for commands." />
      <RunTask />
      <div className="grid grid-cols-2 gap-5">
        <Section title="Today" actions={<Button variant="ghost" className="h-6 px-2 text-[12px]" onClick={() => setPage("todo")}>All</Button>}>
          {today.length === 0 ? (
            <Empty>Nothing on your list for today.</Empty>
          ) : (
            today.map((t) => (
              <label key={t.id} className="flex items-center gap-2.5 py-2 border-b border-[var(--border)] last:border-b-0 cursor-pointer">
                <input type="checkbox" className="h-4 w-4 accent-[var(--accent)]" checked={t.done} onChange={() => ipc.todosUpdate({ id: t.id, done: !t.done })} />
                <span className="flex-1 truncate">{t.title}</span>
                {t.due_at && <span className="text-[12px] text-[var(--muted)]">{whenLabel(t.due_at)}</span>}
              </label>
            ))
          )}
        </Section>
        <Section title="Next routines" actions={<Button variant="ghost" className="h-6 px-2 text-[12px]" onClick={() => setPage("routines")}>All</Button>}>
          {next.length === 0 ? (
            <Empty>No routines scheduled.</Empty>
          ) : (
            next.map((j) => (
              <div key={j.id} className="flex items-center gap-2.5 py-2 border-b border-[var(--border)] last:border-b-0">
                <span className="flex-1 truncate">{j.name}</span>
                <span className="text-[12px] text-[var(--muted)]">{whenLabel(j.next_run_at)}</span>
              </div>
            ))
          )}
        </Section>
      </div>
      <Section title="Recent commands" actions={<Button variant="ghost" className="h-6 px-2 text-[12px]" onClick={() => setPage("history")}>History</Button>}>
        {history.length === 0 ? (
          <Empty>Commands you give Wisp show up here.</Empty>
        ) : (
          history.map((h) => (
            <div key={h.id} className="flex items-center gap-3 py-2 border-b border-[var(--border)] last:border-b-0">
              <Badge>{h.source.replace("_", " ")}</Badge>
              <span className="flex-1 truncate">{truncate(h.transcript ?? "", 70)}</span>
              <span className="text-[12px] text-[var(--muted)] truncate max-w-[220px]">{truncate(h.outcome, 40)}</span>
              <span className="text-[12px] text-[var(--muted)] w-[72px] text-right">{ago(h.at)}</span>
            </div>
          ))
        )}
      </Section>
      <Section title="Gemini">
        <div className="flex items-center gap-3 py-2.5">
          <KeyRound size={16} className="text-[var(--muted)]" />
          <span className="flex-1">
            {status?.has_key ? (
              <>
                Key stored <span className="text-[var(--muted)]">{status.key_masked}</span>
              </>
            ) : (
              <span className="text-[var(--danger)]">No key. Add one in Settings.</span>
            )}
          </span>
          <span className="text-[12px] text-[var(--muted)]">
            {status?.requests_today ?? 0} requests today · {status?.rate_limited_today ?? 0} rate limited
          </span>
        </div>
      </Section>
    </>
  );
}

/** FR-CU-01: Run task box. */
function RunTask() {
  const [goal, setGoal] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const run = async () => {
    if (!goal.trim()) return;
    setBusy(true);
    setResult(null);
    try {
      const r = await ipc.runAgentTask(goal.trim());
      setResult(r);
      if (r.ok) setGoal("");
    } catch (e) {
      setResult({ ok: false, text: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mb-6 rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] p-4">
      <div className="flex items-center gap-2 mb-2.5 text-[13px] font-medium">
        <Bot size={16} className="text-[var(--accent)]" /> Run a task
        <span className="text-[var(--muted)] font-normal">Wisp operates your PC with mouse and keyboard. Esc ×3 stops it.</span>
      </div>
      <div className="flex gap-2">
        <Input
          aria-label="Task goal"
          className="flex-1"
          placeholder="Open Notepad and type hello world"
          value={goal}
          maxLength={1000}
          onChange={(e) => setGoal(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
        />
        <Button variant="primary" busy={busy} disabled={!goal.trim()} onClick={run}>
          <Play size={14} /> Run
        </Button>
        {busy && (
          <Button variant="danger" onClick={() => ipc.agentStop()}>
            Stop
          </Button>
        )}
      </div>
      {result && <div className={`mt-2 text-[13px] ${result.ok ? "text-[var(--ok)]" : "text-[var(--danger)]"}`}>{result.text}</div>}
    </div>
  );
}
