import { ChevronDown, ChevronRight, Copy } from "lucide-react";
import { useState } from "react";
import { useReload } from "../../shared/events";
import { ipc } from "../../shared/ipc";
import { ago, duration, truncate } from "../../shared/format";
import type { CommandEntry, DictationEntry } from "../../shared/types";
import { Badge, Empty, IconButton, Section, Toggle } from "../../shared/ui";
import { PageTitle } from "../App";
import { useMain } from "../store";

/** History (FR-CMD-09, FR-DIC-09): commands with expandable steps, dictations with Copy. */
export function HistoryPage() {
  const { settings, set } = useMain();
  const [commands, setCommands] = useState<CommandEntry[]>([]);
  const [dictations, setDictations] = useState<DictationEntry[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [copied, setCopied] = useState<number | null>(null);
  useReload(["history-changed", "settings-changed"], () => {
    ipc.historyCommands().then(setCommands).catch(() => {});
    ipc.historyDictations().then(setDictations).catch(() => {});
  });

  return (
    <>
      <PageTitle title="History" sub="Kept on this PC only. Commands for 90 days, dictations for 7 days." />
      <Section title="Commands">
        {commands.length === 0 && <Empty>No commands yet.</Empty>}
        {commands.map((c) => {
          const expanded = open === c.id;
          const tools = c.tools ?? [];
          const hasDetail = tools.length > 0 || c.steps.length > 0;
          return (
            <div key={c.id} className="border-b border-[var(--border)] last:border-b-0">
              <button
                type="button"
                aria-expanded={expanded}
                disabled={!hasDetail}
                onClick={() => setOpen(expanded ? null : c.id)}
                className="w-full flex items-center gap-3 py-2.5 text-left"
              >
                <span className="w-4 text-[var(--muted)]">{hasDetail ? expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} /> : null}</span>
                <Badge>{c.source.replace("_", " ")}</Badge>
                <span className="flex-1 min-w-0 truncate">{c.transcript}</span>
                <span className={`text-[12px] truncate max-w-[230px] ${c.outcome.startsWith("error") || c.outcome.startsWith("failed") ? "text-[var(--danger)]" : "text-[var(--muted)]"}`}>
                  {truncate(c.outcome, 50)}
                </span>
                <span className="text-[12px] text-[var(--muted)] w-[54px] text-right">{duration(c.duration_ms)}</span>
                <span className="text-[12px] text-[var(--muted)] w-[80px] text-right">{ago(c.at)}</span>
              </button>
              {expanded && (
                <div className="pl-7 pb-3 text-[12px] flex flex-col gap-1.5">
                  {tools.map((t, i) => (
                    <div key={i} className="flex gap-2">
                      <Badge tone={t.ok ? "ok" : "danger"}>{t.name}</Badge>
                      <code className="text-[var(--muted)] truncate">{JSON.stringify(t.args)}</code>
                      {t.error && <span className="text-[var(--danger)]">{t.error}</span>}
                    </div>
                  ))}
                  {c.steps.map((s) => (
                    <div key={s.step} className="flex gap-2 items-baseline">
                      <span className="text-[var(--muted)] w-6">#{s.step}</span>
                      <span className="flex-1">{s.intent}</span>
                      <code className="text-[var(--muted)] truncate max-w-[280px]">{JSON.stringify(s.action)}</code>
                      {s.safety && <Badge tone="warn">{s.safety}</Badge>}
                      {s.user_answer && <Badge tone={s.user_answer === "yes" ? "ok" : "danger"}>{s.user_answer}</Badge>}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </Section>
      <Section
        title="Dictations"
        actions={
          settings && (
            <label className="flex items-center gap-2 text-[12px] text-[var(--muted)]">
              Keep history
              <Toggle label="Keep dictation history" checked={settings.dictation.history_enabled} onChange={(v) => set("dictation.history_enabled", v)} />
            </label>
          )
        }
      >
        {settings && !settings.dictation.history_enabled && <Empty>Dictation history is off.</Empty>}
        {settings?.dictation.history_enabled && dictations.length === 0 && <Empty>Your last 50 dictations show up here.</Empty>}
        {dictations.map((d) => (
          <div key={d.id} className="flex items-center gap-3 py-2 border-b border-[var(--border)] last:border-b-0">
            <span className="flex-1 min-w-0 truncate">{d.text}</span>
            <span className="text-[12px] text-[var(--muted)]">{copied === d.id ? "Copied" : ago(d.at)}</span>
            <IconButton
              label="Copy"
              onClick={async () => {
                await ipc.copyDictation(d.id);
                setCopied(d.id);
                window.setTimeout(() => setCopied(null), 1200);
              }}
            >
              <Copy size={14} />
            </IconButton>
          </div>
        ))}
      </Section>
    </>
  );
}
