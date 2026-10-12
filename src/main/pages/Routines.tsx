import { Pencil, Play, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { useReload } from "../../shared/events";
import { errorMessage, ipc } from "../../shared/ipc";
import { ago, describeWhen, duration, whenLabel } from "../../shared/format";
import type { Job, JobRun, When, WhenKind } from "../../shared/types";
import { Badge, Button, Dialog, Empty, ErrorText, IconButton, Input, Section, Select, Toggle } from "../../shared/ui";
import { PageTitle } from "../App";

const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** Routines (section 4.6, FR-SCH-10): list, form, run now, 30 days of history. */
export function RoutinesPage() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [runs, setRuns] = useState<JobRun[]>([]);
  const [editing, setEditing] = useState<Job | "new" | null>(null);
  const [err, setErr] = useState("");
  useReload(["jobs-changed"], () => {
    ipc.jobsList().then(setJobs).catch(() => {});
    ipc.jobRunsList().then(setRuns).catch(() => {});
  });

  const toggle = async (j: Job) => {
    try {
      await ipc.jobsUpdate(j.id, { name: j.name, command: j.command, when: j.when, allow_computer_use: j.allow_computer_use, enabled: !j.enabled });
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  return (
    <>
      <PageTitle
        title="Routines"
        sub='Say "every morning at 9, open Unity and Spotify", or add one here.'
        actions={
          <Button variant="primary" onClick={() => setEditing("new")}>
            <Plus size={14} /> New routine
          </Button>
        }
      />
      <ErrorText>{err}</ErrorText>
      <Section title="Scheduled">
        {jobs.length === 0 && <Empty>No routines yet.</Empty>}
        {jobs.map((j) => (
          <div key={j.id} className="flex items-center gap-3 py-2.5 border-b border-[var(--border)] last:border-b-0">
            <Toggle label={`${j.enabled ? "Disable" : "Enable"} ${j.name}`} checked={j.enabled} onChange={() => toggle(j)} />
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <span className="font-medium truncate">{j.name}</span>
                {j.allow_computer_use && <Badge tone="warn">Computer Use</Badge>}
              </div>
              <div className="text-[12px] text-[var(--muted)] truncate">
                {describeWhen(j.when)} · “{j.command}”
              </div>
            </div>
            <span className="text-[12px] text-[var(--muted)] w-[120px] text-right">{j.next_run_at ? `Next ${whenLabel(j.next_run_at)}` : j.enabled ? "Finished" : "Off"}</span>
            <IconButton label={`Run ${j.name} now`} onClick={() => ipc.jobsRunNow(j.id).catch((e) => setErr(errorMessage(e)))}>
              <Play size={14} />
            </IconButton>
            <IconButton label={`Edit ${j.name}`} onClick={() => setEditing(j)}>
              <Pencil size={14} />
            </IconButton>
            <IconButton label={`Delete ${j.name}`} onClick={() => ipc.jobsDelete(j.id)}>
              <Trash2 size={14} />
            </IconButton>
          </div>
        ))}
      </Section>
      <Section title="Run history (30 days)">
        {runs.length === 0 && <Empty>No runs yet.</Empty>}
        {runs.slice(0, 100).map((r) => (
          <div key={r.id} className="flex items-center gap-3 py-2 border-b border-[var(--border)] last:border-b-0 text-[13px]">
            <Badge tone={r.status === "ok" ? "ok" : r.status === "missed" ? "warn" : "danger"}>{r.status}</Badge>
            <span className="w-[140px] truncate">{r.job_name ?? `#${r.job_id}`}</span>
            <span className="flex-1 truncate text-[var(--muted)]">{r.summary}</span>
            <span className="text-[12px] text-[var(--muted)] w-[60px] text-right">{duration(r.duration_ms)}</span>
            <span className="text-[12px] text-[var(--muted)] w-[90px] text-right">{ago(r.due_at)}</span>
          </div>
        ))}
      </Section>
      {editing && <JobForm job={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
    </>
  );
}

function defaultWhen(): When {
  const d = new Date(Date.now() + 3600_000);
  const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  return { kind: "daily", time: "09:00", date, days: ["mon"], day_of_month: 1, cron: "0 9 * * 1-5" };
}

/** Builds the `when` object with only the fields its kind needs (section 5.5). */
export function cleanWhen(w: When): When {
  switch (w.kind) {
    case "once":
      return { kind: "once", date: w.date, time: w.time };
    case "daily":
    case "weekdays":
      return { kind: w.kind, time: w.time };
    case "weekly":
      return { kind: "weekly", days: w.days, time: w.time };
    case "monthly":
      return { kind: "monthly", day_of_month: w.day_of_month, time: w.time };
    case "cron":
      return { kind: "cron", cron: w.cron };
  }
}

function JobForm({ job, onClose }: { job: Job | null; onClose: () => void }) {
  const [name, setName] = useState(job?.name ?? "");
  const [command, setCommand] = useState(job?.command ?? "");
  const [when, setWhen] = useState<When>({ ...defaultWhen(), ...(job?.when ?? {}) });
  const [allowCu, setAllowCu] = useState(job?.allow_computer_use ?? false);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const patch = (p: Partial<When>) => setWhen((w) => ({ ...w, ...p }));

  const save = async () => {
    setBusy(true);
    setErr("");
    const body = { name: name.trim(), command: command.trim(), when: cleanWhen(when), allow_computer_use: allowCu };
    try {
      if (job) await ipc.jobsUpdate(job.id, { ...body, enabled: job.enabled });
      else await ipc.jobsCreate(body);
      onClose();
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={job ? "Edit routine" : "New routine"}
      onClose={onClose}
      actions={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" busy={busy} onClick={save} disabled={!name.trim() || !command.trim()}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <label className="flex flex-col gap-1">
          <span className="text-[12px] text-[var(--muted)]">Name</span>
          <Input value={name} maxLength={80} onChange={(e) => setName(e.target.value)} placeholder="Morning setup" />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[12px] text-[var(--muted)]">What Wisp should do</span>
          <Input value={command} maxLength={500} onChange={(e) => setCommand(e.target.value)} placeholder="Open Unity and Spotify" />
        </label>
        <div className="flex gap-2 items-end flex-wrap">
          <label className="flex flex-col gap-1">
            <span className="text-[12px] text-[var(--muted)]">Repeat</span>
            <Select value={when.kind} onChange={(e) => patch({ kind: e.target.value as WhenKind })}>
              <option value="once">Once</option>
              <option value="daily">Daily</option>
              <option value="weekdays">Weekdays</option>
              <option value="weekly">Weekly</option>
              <option value="monthly">Monthly</option>
              <option value="cron">Advanced (cron)</option>
            </Select>
          </label>
          {when.kind === "once" && (
            <label className="flex flex-col gap-1">
              <span className="text-[12px] text-[var(--muted)]">Date</span>
              <Input type="date" value={when.date ?? ""} onChange={(e) => patch({ date: e.target.value })} />
            </label>
          )}
          {when.kind === "monthly" && (
            <label className="flex flex-col gap-1">
              <span className="text-[12px] text-[var(--muted)]">Day</span>
              <Input type="number" min={1} max={31} className="w-[70px]" value={when.day_of_month ?? 1} onChange={(e) => patch({ day_of_month: Number(e.target.value) })} />
            </label>
          )}
          {when.kind !== "cron" ? (
            <label className="flex flex-col gap-1">
              <span className="text-[12px] text-[var(--muted)]">Time</span>
              <Input type="time" value={when.time ?? ""} onChange={(e) => patch({ time: e.target.value })} />
            </label>
          ) : (
            <label className="flex flex-col gap-1 flex-1">
              <span className="text-[12px] text-[var(--muted)]">Cron (minute hour day month weekday)</span>
              <Input value={when.cron ?? ""} onChange={(e) => patch({ cron: e.target.value })} placeholder="0 9 * * 1-5" />
            </label>
          )}
        </div>
        {when.kind === "weekly" && (
          <div className="flex gap-1" role="group" aria-label="Days">
            {DAYS.map((d) => {
              const on = when.days?.includes(d);
              return (
                <button
                  key={d}
                  type="button"
                  aria-pressed={on}
                  onClick={() => patch({ days: on ? when.days!.filter((x) => x !== d) : [...(when.days ?? []), d] })}
                  className={`h-8 w-11 rounded-[var(--radius)] border text-[12px] capitalize ${on ? "bg-[var(--accent)] text-[#0b0b10] border-transparent" : "border-[var(--border)]"}`}
                >
                  {d}
                </button>
              );
            })}
          </div>
        )}
        <label className="flex items-start gap-2.5 mt-1 cursor-pointer">
          <input type="checkbox" className="h-4 w-4 mt-0.5 accent-[var(--accent)]" checked={allowCu} onChange={(e) => setAllowCu(e.target.checked)} />
          <span>
            Allow Computer Use
            <span className="block text-[12px] text-[var(--muted)]">Lets this routine control the mouse and keyboard while you are away. Confirmations still wait for you, and give up after 10 minutes.</span>
          </span>
        </label>
        <ErrorText>{err}</ErrorText>
      </div>
    </Dialog>
  );
}
