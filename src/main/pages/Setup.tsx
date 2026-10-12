import { CheckCircle2, ExternalLink, KeyRound, Mic, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useEvent } from "../../shared/events";
import { errorMessage, ipc } from "../../shared/ipc";
import { Button, ErrorText, Input } from "../../shared/ui";
import { WispMark } from "../App";
import { useMain } from "../store";

export const PRIVACY_TEXT =
  "When you use Wisp, the audio you record, screenshots you share and clipboard text sent for a request go to Google's Gemini API. On the free tier, Google may use that data to improve its products. Nothing else leaves your PC: Wisp has no telemetry, analytics or update checks, and your key stays in Windows Credential Manager.";

const STEPS = ["Privacy", "Get a key", "Add your key", "Microphone"];

/** First-run wizard (FR-SET-01 to 06). */
export function SetupWizard({ onDone }: { onDone: () => void }) {
  const { status, refresh, set } = useMain();
  const [step, setStep] = useState(status?.privacy_ack_at ? (status.has_key ? 3 : 1) : 0);
  const [ack, setAck] = useState(!!status?.privacy_ack_at);

  const finish = async () => {
    await set("app.first_run_done", true);
    await refresh();
    onDone();
  };

  return (
    <div className="w-[560px] max-w-[94vw] min-h-[520px] rounded-[calc(var(--radius)+6px)] border border-[var(--border)] bg-[var(--panel)] p-7 flex flex-col shadow-xl">
      <div className="flex items-center gap-2.5 mb-5">
        <WispMark size={26} />
        <div>
          <div className="font-semibold text-[16px]">Set up Wisp</div>
          <div className="text-[12px] text-[var(--muted)]">
            Step {step + 1} of 4 · {STEPS[step]}
          </div>
        </div>
      </div>
      <ol className="flex gap-1.5 mb-6" aria-label="Progress">
        {STEPS.map((s, i) => (
          <li key={s} className={`h-1 flex-1 rounded-full ${i <= step ? "bg-[var(--accent)]" : "bg-[var(--border)]"}`} aria-label={`${s}${i < step ? " done" : ""}`} />
        ))}
      </ol>
      <div className="flex-1">
        {step === 0 && (
          <div>
            <Heading icon={<ShieldCheck size={18} />} title="Before you start" />
            <p className="text-[14px] leading-relaxed">{PRIVACY_TEXT}</p>
            <label className="flex items-center gap-2.5 mt-5 text-[14px] cursor-pointer">
              <input type="checkbox" className="h-4 w-4 accent-[var(--accent)]" checked={ack} onChange={(e) => setAck(e.target.checked)} />I understand
            </label>
          </div>
        )}
        {step === 1 && (
          <div>
            <Heading icon={<ExternalLink size={18} />} title="Get a Gemini API key" />
            <p className="text-[14px] leading-relaxed">
              Wisp uses your own key from Google AI Studio. The free tier is enough to start. Sign in, create a key, then copy it.
            </p>
            <Button className="mt-5" variant="primary" onClick={() => ipc.openExternal("aistudio")}>
              <ExternalLink size={14} /> Open Google AI Studio
            </Button>
            <p className="text-[12px] text-[var(--muted)] mt-3">This opens aistudio.google.com/apikey in your browser.</p>
          </div>
        )}
        {step === 2 && <KeyStep onSaved={() => setStep(3)} />}
        {step === 3 && <MicStep />}
      </div>
      <div className="flex justify-between mt-6">
        <Button variant="ghost" disabled={step === 0} onClick={() => setStep((s) => s - 1)}>
          Back
        </Button>
        {step === 0 && (
          <Button
            variant="primary"
            disabled={!ack}
            onClick={async () => {
              await set("app.privacy_ack_at", new Date().toISOString());
              setStep(1);
            }}
          >
            Continue
          </Button>
        )}
        {step === 1 && (
          <Button variant="primary" onClick={() => setStep(2)}>
            I have a key
          </Button>
        )}
        {step === 2 && status?.has_key && (
          <Button variant="primary" onClick={() => setStep(3)}>
            Continue
          </Button>
        )}
        {step === 3 && (
          <Button variant="primary" onClick={finish} disabled={!status?.has_key}>
            Finish
          </Button>
        )}
      </div>
    </div>
  );
}

function Heading({ icon, title }: { icon: React.ReactNode; title: string }) {
  return (
    <h2 className="flex items-center gap-2 text-[17px] font-semibold mb-3">
      <span className="text-[var(--accent)]">{icon}</span>
      {title}
    </h2>
  );
}

/** FR-SET-04/05: validate via the models list, then store in Credential Manager. */
export function KeyStep({ onSaved }: { onSaved: () => void }) {
  const { refresh } = useMain();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const validate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const r = await ipc.validateKey(key.trim());
      if (r.ok) {
        await ipc.secretsSetKey(key.trim());
        setKey("");
        await refresh();
        setResult({ ok: true, text: `Key works. ${r.model_count} models are available.` });
        window.setTimeout(onSaved, 900);
      } else {
        setResult({ ok: false, text: `${r.status ? `HTTP ${r.status}. ` : ""}${r.cause ?? "The key could not be checked."}` });
      }
    } catch (e) {
      setResult({ ok: false, text: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <Heading icon={<KeyRound size={18} />} title="Paste your key" />
      <p className="text-[14px] leading-relaxed mb-4">Wisp checks the key with Google, then stores it in Windows Credential Manager. It never appears in files or logs.</p>
      <div className="flex gap-2">
        <Input
          type="password"
          aria-label="Gemini API key"
          placeholder="AIza…"
          autoComplete="off"
          spellCheck={false}
          className="flex-1"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && key.trim() && validate()}
        />
        <Button variant="primary" busy={busy} disabled={!key.trim()} onClick={validate}>
          Validate
        </Button>
      </div>
      {result && (
        <div role="status" className={`mt-3 text-[13px] flex items-start gap-2 ${result.ok ? "text-[var(--ok)]" : "text-[var(--danger)]"}`}>
          {result.ok && <CheckCircle2 size={16} className="shrink-0 mt-px" />}
          {result.text}
        </div>
      )}
    </div>
  );
}

/** FR-SET-06: live level meter and a 3 s record-and-play test. */
export function MicStep() {
  const [level, setLevel] = useState(0);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEvent<{ rms: number }>("audio-level", (p) => setLevel(p.rms));

  const test = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const r = await ipc.micTest();
      setMsg({ ok: r.ok, text: r.message });
    } catch (e) {
      setMsg({ ok: false, text: errorMessage(e) });
    } finally {
      setBusy(false);
      setLevel(0);
    }
  };

  return (
    <div>
      <Heading icon={<Mic size={18} />} title="Test your microphone" />
      <p className="text-[14px] leading-relaxed mb-4">Press Test and say something. Wisp records 3 seconds and plays it back.</p>
      <div className="h-2.5 rounded-full bg-[var(--panel-2)] overflow-hidden mb-4" role="meter" aria-label="Microphone level" aria-valuenow={Math.round(level * 100)} aria-valuemin={0} aria-valuemax={100}>
        <div className="h-full bg-[var(--accent)] transition-[width] duration-75" style={{ width: `${Math.min(100, level * 100)}%` }} />
      </div>
      <Button variant="primary" busy={busy} onClick={test}>
        <Mic size={14} /> {busy ? "Recording…" : "Test"}
      </Button>
      {msg && <div className={`mt-3 text-[13px] ${msg.ok ? "text-[var(--ok)]" : "text-[var(--danger)]"}`}>{msg.text}</div>}
      {msg && !msg.ok && (
        <Button className="mt-3" onClick={() => ipc.openExternal("mic_privacy")}>
          Open microphone privacy settings
        </Button>
      )}
      <ErrorText>{null}</ErrorText>
      <p className="text-[12px] text-[var(--muted)] mt-5">
        Then try it: hold <b>Ctrl + Win</b> in any text box and speak.
      </p>
    </div>
  );
}
