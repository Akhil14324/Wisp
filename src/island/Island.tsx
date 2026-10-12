import { CornerDownLeft, Hand, Square, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useEvent } from "../shared/events";
import { errorMessage, ipc } from "../shared/ipc";
import { useSkin } from "../shared/theme";
import type { AppEntry, ConfirmRequest, IslandEvent } from "../shared/types";
import { Panel } from "./Panel";
import { BARS, useIsland } from "./store";

const PILL_W = 140;
const WIDE_W = 420;
const HOVER_EXPAND_MS = 400;
const LEAVE_COLLAPSE_MS = 800;

/** Island widget (section 4.9). Rust sizes and places the window; we request sizes. */
export function Island() {
  useSkin();
  const s = useIsland();
  const rootRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef<number>();
  const leaveTimer = useRef<number>();
  const jobTimer = useRef<number>();
  const [typed, setTyped] = useState("");
  const [inputError, setInputError] = useState("");
  const [busy, setBusy] = useState(false);

  useEvent<IslandEvent>("island-state", (ev) => {
    s.setEvent(ev);
    if (ev.state !== "idle" && s.view === "expanded") s.setView("collapsed");
    if (ev.state === "idle" || ev.state === "listening") s.setChoices([]);
  });
  useEvent<{ rms: number }>("audio-level", (p) => s.pushLevel(p.rms));
  useEvent<ConfirmRequest>("confirm-request", (c) => {
    s.setConfirm(c);
    ipc.islandSetInteractive(true).catch(() => {});
  });
  useEvent<{ id: number }>("confirm-closed", (p) => {
    if (useIsland.getState().confirm?.id === p.id) {
      s.setConfirm(null);
      ipc.islandSetInteractive(false).catch(() => {});
    }
  });
  useEvent<{ paused: boolean }>("agent-paused", (p) => s.setPaused(p.paused));
  useEvent<{ choices: AppEntry[] }>("app-choices", (p) => s.setChoices(p.choices.slice(0, 3)));
  useEvent<{ name: string; when: string }>("job-created", (p) => {
    s.setJob({ name: p.name, when: p.when });
    window.clearTimeout(jobTimer.current);
    jobTimer.current = window.setTimeout(() => useIsland.getState().setJob(null), 6000);
  });

  const { ev, view, confirm, choices, job, paused } = s;
  const active = ev.state !== "idle";
  const showCard = !!confirm || paused || choices.length > 0 || !!job;
  const wide = view !== "collapsed" || ev.controlling || showCard || ["acting", "result", "error"].includes(ev.state);
  const width = wide ? WIDE_W : PILL_W;

  // Ask Rust for a window that fits the content.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const send = () => ipc.islandLayout(width, Math.ceil(el.getBoundingClientRect().height)).catch(() => {});
    send();
    const ro = new ResizeObserver(send);
    ro.observe(el);
    return () => ro.disconnect();
  }, [width, view]);

  const expand = useCallback(() => {
    if (useIsland.getState().ev.state === "idle" && useIsland.getState().view === "collapsed") s.setView("expanded");
  }, [s]);

  const onEnter = () => {
    window.clearTimeout(leaveTimer.current);
    if (view === "collapsed" && !active && !showCard) hoverTimer.current = window.setTimeout(expand, HOVER_EXPAND_MS);
  };
  const onLeave = () => {
    window.clearTimeout(hoverTimer.current);
    if (view === "expanded") leaveTimer.current = window.setTimeout(() => useIsland.getState().setView("collapsed"), LEAVE_COLLAPSE_MS);
  };
  const onWheel = (e: React.WheelEvent) => {
    if (view === "collapsed" && e.deltaY > 0 && !active) expand();
    else if (view === "expanded") s.cycleTab(e.deltaY > 0 ? 1 : -1);
  };

  const openInput = () => {
    if (active || showCard) return;
    window.clearTimeout(hoverTimer.current);
    s.setView("input");
    setInputError("");
    ipc.islandSetInteractive(true).catch(() => {});
  };
  const closeInput = useCallback(() => {
    s.setView("collapsed");
    setTyped("");
    ipc.islandSetInteractive(false).catch(() => {});
  }, [s]);

  const submit = async () => {
    const text = typed.trim();
    if (!text || busy) return;
    setBusy(true);
    try {
      closeInput();
      await ipc.runTextCommand(text);
    } catch (e) {
      setInputError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const answer = async (yes: boolean) => {
    const c = useIsland.getState().confirm;
    if (!c) return;
    s.setConfirm(null);
    ipc.islandSetInteractive(false).catch(() => {});
    try {
      await ipc.confirmAnswer(c.id, yes);
    } catch {
      /* expired */
    }
  };

  return (
    <div className="island w-full flex justify-center" onMouseEnter={onEnter} onMouseLeave={onLeave} onWheel={onWheel}>
      <div ref={rootRef} style={{ width }} className="flex flex-col items-stretch">
        <div aria-live="polite" className="sr-only">
          {ev.text ?? ""}
        </div>
        {ev.controlling && <ControlBanner />}
        {confirm ? (
          <ConfirmCard req={confirm} onAnswer={answer} />
        ) : paused ? (
          <PausedCard />
        ) : choices.length > 0 ? (
          <ChoicesCard choices={choices} onDone={() => s.setChoices([])} />
        ) : job ? (
          <JobCard name={job.name} when={job.when} onClose={() => s.setJob(null)} />
        ) : view === "input" ? (
          <TypedBox value={typed} onChange={setTyped} onSubmit={submit} onCancel={closeInput} error={inputError} />
        ) : view === "expanded" ? (
          <Panel />
        ) : (
          <Pill ev={ev} onClick={openInput} />
        )}
      </div>
    </div>
  );
}

function Pill({ ev, onClick }: { ev: IslandEvent; onClick: () => void }) {
  const levels = useIsland((s) => s.levels);
  const wide = ["acting", "result", "error"].includes(ev.state);
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={ev.state === "idle" ? "Wisp. Click to type a command" : (ev.text ?? ev.state)}
      className={`island-shell island-pill island-enter flex items-center gap-2.5 px-3.5 ${wide ? "justify-start min-h-[34px] py-1.5" : "justify-center h-[34px]"}`}
    >
      {ev.state === "idle" && <span className="island-dot" />}
      {ev.state === "listening" && (
        <span className="flex items-center gap-[3px] h-5" aria-hidden>
          {levels.map((l, i) => (
            <span key={i} className="island-bar" style={{ height: `${4 + Math.min(1, l * (0.6 + 0.4 * Math.sin((i / BARS) * Math.PI))) * 16}px` }} />
          ))}
        </span>
      )}
      {ev.state === "thinking" && (
        <>
          <span className="island-dot thinking" />
          <span className="muted">Thinking</span>
        </>
      )}
      {ev.state === "confirm" && <span className="muted">{ev.text ?? "Waiting for you"}</span>}
      {ev.state === "acting" && (
        <>
          <span className="island-spinner shrink-0" />
          <span className="truncate text-left">{ev.text}</span>
        </>
      )}
      {ev.state === "result" && <span className="text-left leading-snug line-clamp-4 break-words">{ev.text}</span>}
      {ev.state === "error" && <span className="text-left leading-snug text-[#ff8a8e] line-clamp-3">{ev.text}</span>}
    </button>
  );
}

function ControlBanner() {
  return (
    <div className="island-shell island-pill island-enter flex items-center justify-between gap-2 pl-3.5 pr-1 h-[34px] mb-1.5">
      <span className="text-[12px] truncate">Wisp is controlling your PC. Esc x3 to stop</span>
      <button type="button" className="island-btn danger h-[26px] px-2.5" onClick={() => ipc.agentStop()}>
        <Square size={12} /> Stop
      </button>
    </div>
  );
}

/** Confirmation card (section 8.5): No has default focus, Esc means No, never auto-Yes. */
function ConfirmCard({ req, onAnswer }: { req: ConfirmRequest; onAnswer: (yes: boolean) => void }) {
  const noRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    noRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onAnswer(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [req.id, onAnswer]);
  const heading = req.kind === "first_task" ? "Allow PC control" : req.kind === "guard" ? "Guarded window" : "Confirm this step";
  return (
    <div role="alertdialog" aria-label={heading} className="island-shell island-panel island-enter p-4">
      <div className="text-[11px] uppercase tracking-wide muted mb-1">{heading}</div>
      <div className="font-semibold leading-snug break-words">{req.title}</div>
      {req.detail && <div className="muted mt-1.5 leading-snug whitespace-pre-line break-words text-[12px]">{req.detail}</div>}
      <div className="flex justify-end gap-2 mt-3.5">
        <button ref={noRef} type="button" className="island-btn" onClick={() => onAnswer(false)}>
          No
        </button>
        <button type="button" className="island-btn primary" onClick={() => onAnswer(true)}>
          Yes
        </button>
      </div>
    </div>
  );
}

function PausedCard() {
  return (
    <div className="island-shell island-panel island-enter p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Hand size={15} /> Paused: you took over
      </div>
      <div className="muted mt-1 text-[12px]">Wisp stopped because you used the keyboard or mouse.</div>
      <div className="flex justify-end gap-2 mt-3">
        <button type="button" className="island-btn danger" onClick={() => ipc.agentStop()}>
          Stop
        </button>
        <button type="button" className="island-btn primary" onClick={() => ipc.agentResume()}>
          Resume
        </button>
      </div>
    </div>
  );
}

function ChoicesCard({ choices, onDone }: { choices: AppEntry[]; onDone: () => void }) {
  return (
    <div className="island-shell island-panel island-enter p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="muted text-[12px]">Which one?</span>
        <button type="button" className="island-icon-btn h-6 w-6" aria-label="Dismiss" onClick={onDone}>
          <X size={13} />
        </button>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {choices.map((c) => (
          <button
            key={c.id}
            type="button"
            className="island-btn"
            onClick={async () => {
              onDone();
              await ipc.launchApp(c.id).catch(() => {});
            }}
          >
            {c.name}
          </button>
        ))}
      </div>
    </div>
  );
}

function JobCard({ name, when, onClose }: { name: string; when: string; onClose: () => void }) {
  return (
    <div className="island-shell island-panel island-enter p-3 flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <div className="text-[11px] uppercase tracking-wide muted">Scheduled</div>
        <div className="font-semibold truncate">{name}</div>
        <div className="muted text-[12px]">{when}</div>
      </div>
      <button type="button" className="island-btn h-[26px] px-2.5" onClick={() => ipc.windowShowMain("routines")}>
        View
      </button>
      <button type="button" className="island-icon-btn h-6 w-6" aria-label="Dismiss" onClick={onClose}>
        <X size={13} />
      </button>
    </div>
  );
}

function TypedBox({
  value,
  onChange,
  onSubmit,
  onCancel,
  error,
}: {
  value: string;
  onChange: (v: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
  error: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const t = window.setTimeout(() => ref.current?.focus(), 30);
    return () => window.clearTimeout(t);
  }, []);
  return (
    <div className="island-shell island-panel island-enter p-1.5">
      <div className="flex items-center gap-1.5">
        <input
          ref={ref}
          type="text"
          aria-label="Type a command"
          placeholder="Ask Wisp to do something…"
          className="flex-1 !h-8 !bg-transparent !border-0"
          value={value}
          maxLength={2000}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onSubmit();
            if (e.key === "Escape") onCancel();
          }}
          onBlur={onCancel}
        />
        <button type="button" className="island-icon-btn" aria-label="Run" onMouseDown={(e) => e.preventDefault()} onClick={onSubmit}>
          <CornerDownLeft size={15} />
        </button>
      </div>
      {error && <div className="text-[#ff8a8e] text-[12px] px-2 pb-1">{error}</div>}
    </div>
  );
}
