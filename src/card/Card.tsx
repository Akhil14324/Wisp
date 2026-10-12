import { Check, Copy, CornerDownLeft, MessageSquarePlus, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useEvent } from "../shared/events";
import { errorMessage, ipc } from "../shared/ipc";
import { Markdown } from "../shared/markdown";
import { useSkin } from "../shared/theme";
import type { CardUpdate } from "../shared/types";

/** Circle to Ask result card (FR-CTA-06/07): Markdown answer, Copy, Follow-up, Close. */
export function Card() {
  useSkin();
  const [u, setU] = useState<CardUpdate>({ state: "loading", text: "", can_follow_up: false });
  const [asking, setAsking] = useState(false);
  const [q, setQ] = useState("");
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEvent<CardUpdate>("card-update", (x) => {
    setU(x);
    setErr("");
    setAsking(x.state === "ask");
    if (x.state === "ask") setQ("");
  });

  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const send = () => ipc.cardResize(Math.ceil(el.getBoundingClientRect().height) + 2).catch(() => {});
    send();
    const ro = new ResizeObserver(send);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (asking) inputRef.current?.focus();
  }, [asking]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") ipc.ctaClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const send = async () => {
    const text = q.trim();
    if (!text) return;
    setQ("");
    setAsking(false);
    try {
      await ipc.ctaAsk(text);
    } catch (e) {
      setErr(errorMessage(e));
      setAsking(true);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(u.text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard unavailable */
    }
  };

  return (
    <div className="p-1.5">
      <div
        ref={rootRef}
        role="dialog"
        aria-label="Answer"
        className="rounded-[14px] border border-[var(--border)] bg-[var(--panel)] text-[var(--fg)] shadow-2xl overflow-hidden"
        style={{ maxHeight: 410 }}
      >
        <div className="flex items-center justify-between px-3.5 pt-2.5">
          <span className="text-[11px] uppercase tracking-wide text-[var(--muted)] font-semibold">Wisp</span>
          <button type="button" aria-label="Close" className="h-7 w-7 inline-flex items-center justify-center rounded-md text-[var(--muted)] hover:bg-[var(--panel-2)]" onClick={() => ipc.ctaClose()}>
            <X size={15} />
          </button>
        </div>
        <div className="px-3.5 pb-3 text-[14px] overflow-y-auto" style={{ maxHeight: 300 }} aria-live="polite">
          {u.state === "loading" && (
            <div className="flex items-center gap-2 text-[var(--muted)] py-2">
              <span className="h-2 w-2 rounded-full bg-[var(--accent)]" style={{ animation: "wisp-pulse 1.1s ease-in-out infinite" }} />
              Thinking…
            </div>
          )}
          {u.state === "answer" && <Markdown text={u.text} />}
          {u.state === "error" && <div className="text-[var(--danger)] py-1">{u.text}</div>}
          {u.state === "ask" && !asking && <div className="text-[var(--muted)]">{u.text}</div>}
          {err && <div className="text-[var(--danger)] text-[12px] mt-1">{err}</div>}
        </div>
        {asking ? (
          <div className="flex items-center gap-1.5 border-t border-[var(--border)] px-2 py-1.5">
            <input
              ref={inputRef}
              aria-label="Your question"
              placeholder={u.state === "ask" ? "Type your question…" : "Ask a follow-up…"}
              className="flex-1 h-8 bg-transparent outline-none text-[14px] px-1.5 placeholder:text-[var(--muted)]"
              value={q}
              maxLength={2000}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") send();
                if (e.key === "Escape") {
                  e.stopPropagation();
                  ipc.ctaClose();
                }
              }}
            />
            <button type="button" aria-label="Send" className="h-8 w-8 inline-flex items-center justify-center rounded-md hover:bg-[var(--panel-2)]" onClick={send}>
              <CornerDownLeft size={15} />
            </button>
          </div>
        ) : (
          u.state === "answer" && (
            <div className="flex items-center gap-1 border-t border-[var(--border)] px-2 py-1.5">
              <button type="button" className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-[12px] hover:bg-[var(--panel-2)]" onClick={copy}>
                {copied ? <Check size={13} /> : <Copy size={13} />} {copied ? "Copied" : "Copy"}
              </button>
              {u.can_follow_up && (
                <button type="button" className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-[12px] hover:bg-[var(--panel-2)]" onClick={() => setAsking(true)}>
                  <MessageSquarePlus size={13} /> Follow-up
                </button>
              )}
              <span className="ml-auto text-[11px] text-[var(--muted)] pr-1">Esc to close</span>
            </div>
          )
        )}
      </div>
    </div>
  );
}
