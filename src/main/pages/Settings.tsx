import { AlertTriangle, ClipboardCopy, FolderOpen, Pin, PinOff, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useReload } from "../../shared/events";
import { chordLabel } from "../../shared/format";
import { errorMessage, ipc } from "../../shared/ipc";
import type { AppEntry, ModelsList, MonitorInfo, Pinned, TaskHubStatus } from "../../shared/types";
import { Badge, Button, Dialog, ErrorText, IconButton, Input, Row, Section, Select, Toggle } from "../../shared/ui";
import { PageTitle } from "../App";
import { useMain } from "../store";
import { KeyStep, MicStep, PRIVACY_TEXT } from "./Setup";

export const ALLOWED_CHORDS = ["ctrl+win", "ctrl+shift", "ctrl+alt+space", "ctrl+alt", "alt+shift", "win+alt", "win+shift", "ctrl+win+alt", "ctrl+shift+space"];
export const ENGINES = ["google", "youtube", "amazon", "wikipedia", "bing", "duckduckgo", "github", "maps"];
const SECTIONS = ["General", "Models", "Hotkeys", "Audio", "Privacy", "Safety", "About"] as const;
type SectionId = (typeof SECTIONS)[number];

export function SettingsPage() {
  const [section, setSection] = useState<SectionId>("General");
  return (
    <>
      <PageTitle title="Settings" />
      <div role="tablist" aria-label="Settings sections" className="flex flex-wrap gap-1 mb-5">
        {SECTIONS.map((s) => (
          <button
            key={s}
            role="tab"
            type="button"
            aria-selected={section === s}
            onClick={() => setSection(s)}
            className={`h-8 px-3 rounded-[var(--radius)] text-[13px] ${section === s ? "bg-[var(--panel-2)] font-medium" : "text-[var(--muted)] hover:text-[var(--fg)]"}`}
          >
            {s}
          </button>
        ))}
      </div>
      {section === "General" && <General />}
      {section === "Models" && <Models />}
      {section === "Hotkeys" && <Hotkeys />}
      {section === "Audio" && <Audio />}
      {section === "Privacy" && <Privacy />}
      {section === "Safety" && <Safety />}
      {section === "About" && <About />}
    </>
  );
}

function useMonitors() {
  const [m, setM] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    ipc.monitorsList().then(setM).catch(() => {});
  }, []);
  return m;
}

function MonitorSelect({ value, onChange, label }: { value: string; onChange: (v: string) => void; label: string }) {
  const monitors = useMonitors();
  return (
    <Select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="primary">Primary monitor</option>
      {monitors.map((m, i) => (
        <option key={m.id} value={m.id}>
          {`Monitor ${i + 1}: ${m.width}×${m.height} @ ${Math.round(m.scale * 100)}%${m.primary ? " (primary)" : ""}`}
        </option>
      ))}
    </Select>
  );
}

function General() {
  const { settings, set } = useMain();
  const [folder, setFolder] = useState(settings?.wallpaper.folder ?? "");
  const [err, setErr] = useState("");
  if (!settings) return null;
  return (
    <>
      <TaskHubSection />
      <Section title="Startup">
        <Row label="Start with Windows" hint="Wisp starts in the tray when you sign in.">
          <Toggle label="Start with Windows" checked={settings.app.autostart} onChange={(v) => set("app.autostart", v)} />
        </Row>
      </Section>
      <Section title="Island">
        <Row label="Show the island on">
          <MonitorSelect label="Island monitor" value={settings.island.monitor} onChange={(v) => set("island.monitor", v)} />
        </Row>
        <Row label="Hide over full-screen apps" hint="The island still appears while Wisp is working.">
          <Toggle label="Hide over full-screen apps" checked={settings.island.hide_in_fullscreen} onChange={(v) => set("island.hide_in_fullscreen", v)} />
        </Row>
        <Row label="Wallpaper folder" hint="Images from this folder appear in the island's Wallpapers tab.">
          <Input aria-label="Wallpaper folder" className="w-[260px]" placeholder="C:\Users\you\Pictures\Wallpapers" value={folder} onChange={(e) => setFolder(e.target.value)} />
          <Button
            onClick={async () => {
              setErr("");
              try {
                await ipc.wallpaperPickFolder(folder);
              } catch (e) {
                setErr(errorMessage(e));
              }
            }}
          >
            <FolderOpen size={13} /> Use
          </Button>
        </Row>
        <ErrorText>{err}</ErrorText>
      </Section>
      <PinnedApps />
      <Section title="Search">
        <Row label="Default search engine">
          <Select aria-label="Default search engine" value={settings.search.default_engine} onChange={(e) => set("search.default_engine", e.target.value)}>
            {ENGINES.map((e) => (
              <option key={e} value={e}>
                {e}
              </option>
            ))}
          </Select>
        </Row>
      </Section>
    </>
  );
}

/** TaskHub sign-in: when signed in, Wisp's to-dos are your TaskHub to-dos. */
function TaskHubSection() {
  const [st, setSt] = useState<TaskHubStatus | null>(null);
  const [url, setUrl] = useState("");
  const [user, setUser] = useState("");
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    ipc.taskhubStatus().then((s) => {
      setSt(s);
      setUrl(s.url);
      setUser(s.username ?? "");
    }).catch(() => {});
  }, []);
  if (!st) return null;
  const signIn = async () => {
    setBusy(true);
    setErr("");
    try {
      const s = await ipc.taskhubSignIn(url.trim(), user.trim(), pw);
      setSt(s);
      setPw("");
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Section title="TaskHub">
      {st.signed_in ? (
        <Row label={<>Signed in as <b>{st.username}</b></>} hint="Your to-dos, reminders and voice to-dos now use your TaskHub list: personal to-dos and business tasks assigned to you.">
          <Button
            variant="danger"
            onClick={async () => {
              setSt(await ipc.taskhubSignOut());
            }}
          >
            Sign out
          </Button>
        </Row>
      ) : (
        <div className="py-3 flex flex-col gap-2">
          <p className="text-[12px] text-[var(--muted)]">Sign in with your TaskHub account to use your TaskHub to-dos in Wisp. Your password is kept in Windows Credential Manager.</p>
          <Input aria-label="TaskHub server" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…/api" />
          <div className="flex gap-2">
            <Input aria-label="TaskHub username" className="flex-1" placeholder="Username" autoComplete="off" value={user} onChange={(e) => setUser(e.target.value)} />
            <Input
              aria-label="TaskHub password"
              className="flex-1"
              type="password"
              placeholder="Password"
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && user && pw && signIn()}
            />
            <Button variant="primary" busy={busy} disabled={!user.trim() || !pw} onClick={signIn}>
              Sign in
            </Button>
          </div>
          <ErrorText>{err}</ErrorText>
        </div>
      )}
    </Section>
  );
}

/** FR-ISL-08: choose up to 12 apps from the index. */
function PinnedApps() {
  const [pins, setPins] = useState<Pinned[]>([]);
  const [q, setQ] = useState("");
  const [results, setResults] = useState<AppEntry[]>([]);
  const [err, setErr] = useState("");
  useReload(["pins-changed"], () => {
    ipc.appsPinned().then(setPins).catch(() => {});
  });
  useEffect(() => {
    const t = window.setTimeout(() => {
      if (q.trim()) ipc.appsSearch(q).then(setResults).catch(() => {});
      else setResults([]);
    }, 150);
    return () => window.clearTimeout(t);
  }, [q]);
  return (
    <Section title={`Pinned apps (${pins.length}/12)`}>
      <div className="flex flex-wrap gap-1.5 py-2.5">
        {pins.length === 0 && <span className="text-[13px] text-[var(--muted)]">No pinned apps.</span>}
        {pins.map((p) => (
          <span key={p.app_id} className="inline-flex items-center gap-1 rounded-full border border-[var(--border)] pl-3 pr-1 h-7 text-[13px]">
            {p.label}
            <IconButton label={`Unpin ${p.label}`} className="!h-6 !w-6" onClick={() => ipc.appsUnpin(p.app_id)}>
              <PinOff size={12} />
            </IconButton>
          </span>
        ))}
      </div>
      <div className="pb-3">
        <Input aria-label="Search apps to pin" className="w-full" placeholder="Search installed apps…" value={q} onChange={(e) => setQ(e.target.value)} />
        {results.length > 0 && (
          <div className="mt-1 max-h-[180px] overflow-y-auto rounded-[var(--radius)] border border-[var(--border)]">
            {results.map((a) => (
              <button
                key={a.id}
                type="button"
                className="w-full flex items-center gap-2 px-3 h-8 text-left text-[13px] hover:bg-[var(--panel-2)]"
                onClick={async () => {
                  setErr("");
                  try {
                    await ipc.appsPin(a.id);
                  } catch (e) {
                    setErr(errorMessage(e));
                  }
                }}
              >
                <Pin size={12} className="text-[var(--muted)]" />
                <span className="flex-1 truncate">{a.name}</span>
                {a.kind === "store" && <Badge>Store</Badge>}
              </button>
            ))}
          </div>
        )}
        <ErrorText>{err}</ErrorText>
      </div>
    </Section>
  );
}

/** FR-SET-07: one dropdown per role, warnings on defaults missing from the list. */
function Models() {
  const { settings, set } = useMain();
  const [list, setList] = useState<ModelsList | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    ipc.listModels().then(setList).catch((e) => setErr(errorMessage(e)));
  }, [settings?.models]);
  if (!settings) return null;
  const roles: { key: "dictation" | "command" | "vision" | "computer_use"; label: string; hint: string }[] = [
    { key: "dictation", label: "Dictation", hint: "Cheapest model that accepts audio." },
    { key: "command", label: "Commands", hint: "Voice commands, scheduled jobs, schedule parsing." },
    { key: "vision", label: "Vision", hint: "Circle to Ask and describe_screen." },
    { key: "computer_use", label: "Computer Use", hint: "Recommended: gemini-3.8-flash. Cheaper: gemini-3.5-flash-lite." },
  ];
  const usable = list?.models.filter((m) => m.methods.includes("generateContent") || m.methods.length === 0) ?? [];
  return (
    <Section title="Model per role">
      {err && <div className="py-3 text-[13px] text-[var(--danger)]">{err}</div>}
      {roles.map((r) => {
        const current = settings.models[r.key];
        const missing = list ? !list.models.some((m) => m.id === current) : false;
        return (
          <Row
            key={r.key}
            label={
              <span className="flex items-center gap-2">
                {r.label}
                {missing && (
                  <span className="inline-flex items-center gap-1 text-[12px] text-[var(--warn)]" title="This model is not in your key's model list">
                    <AlertTriangle size={13} /> not available
                  </span>
                )}
              </span>
            }
            hint={r.hint}
          >
            <Select aria-label={`${r.label} model`} className="w-[260px]" value={current} onChange={(e) => set(`models.${r.key}`, e.target.value)}>
              {missing && <option value={current}>{current} (missing)</option>}
              {usable.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}
                </option>
              ))}
              {!list && <option value={current}>{current}</option>}
            </Select>
          </Row>
        );
      })}
      <div className="py-3 text-[12px] text-[var(--muted)]">Pro and image-generation models need a paid plan and are not used.</div>
    </Section>
  );
}

function Hotkeys() {
  const { settings, status, set, refresh } = useMain();
  const [err, setErr] = useState("");
  if (!settings || !status) return null;
  const hk = settings.hotkeys;
  const change = async (k: "dictation" | "circle" | "command", v: string) => {
    setErr("");
    const others = (["dictation", "circle", "command"] as const).filter((x) => x !== k).map((x) => hk[x]);
    if (others.includes(v)) {
      setErr(`${chordLabel(v)} is already used by another action.`);
      return;
    }
    try {
      await set(`hotkeys.${k}`, v);
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  const row = (k: "dictation" | "circle" | "command", label: string, hint: string) => (
    <Row label={label} hint={hint}>
      <Select aria-label={`${label} chord`} value={hk[k]} onChange={(e) => change(k, e.target.value)}>
        {ALLOWED_CHORDS.map((c) => (
          <option key={c} value={c}>
            {chordLabel(c)}
          </option>
        ))}
      </Select>
    </Row>
  );
  return (
    <>
      <Section title="Hold to use">
        {row("dictation", "Dictation", "Speak and the text appears where you are typing.")}
        {row("circle", "Circle to Ask", "Draw around something on screen and ask about it.")}
        {row("command", "Command mode", "Ask Wisp to open apps, search, set reminders and more.")}
        <Row label="Emergency stop" hint="Stops any running task. Fixed.">
          <span className="text-[13px] text-[var(--muted)]">Esc pressed 3 times</span>
        </Row>
        <Row label="Pause all hotkeys" hint="Also in the tray menu.">
          <Toggle
            label="Pause all hotkeys"
            checked={status.hotkeys_paused}
            onChange={async (v) => {
              await ipc.hotkeysSetPaused(v);
              await refresh();
            }}
          />
        </Row>
        <ErrorText>{err}</ErrorText>
      </Section>
      <div className="rounded-[var(--radius)] border border-[color-mix(in_srgb,var(--warn)_40%,transparent)] bg-[color-mix(in_srgb,var(--warn)_8%,transparent)] p-4 text-[13px]">
        <div className="font-medium mb-1">Ctrl + Shift switches keyboard layout?</div>
        If you have several input languages, Windows may switch layout on Ctrl + Shift. Turn that off in Windows Settings, Time &amp; language, Typing, Advanced keyboard settings, Input language hot keys, and set it to Not Assigned.
        <div className="mt-2">
          <Button onClick={() => ipc.openExternal("typing_settings")}>Open typing settings</Button>
        </div>
      </div>
    </>
  );
}

function Audio() {
  const { settings, set } = useMain();
  const [mics, setMics] = useState<string[]>([]);
  useReload(["devices-changed"], () => {
    ipc.micDevicesList().then(setMics).catch(() => {});
  });
  if (!settings) return null;
  return (
    <>
      <Section title="Microphone">
        <Row label="Input device">
          <Select aria-label="Input device" className="w-[280px]" value={settings.audio.input_device ?? ""} onChange={(e) => set("audio.input_device", e.target.value || null)}>
            <option value="">System default</option>
            {mics.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </Select>
        </Row>
      </Section>
      <div className="rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] p-5">
        <MicStep />
      </div>
      <DictionaryEditor />
    </>
  );
}

/** FR-DIC-08: personal dictionary, up to 200 terms. */
function DictionaryEditor() {
  const { settings, set } = useMain();
  const [term, setTerm] = useState("");
  const [err, setErr] = useState("");
  if (!settings) return null;
  const terms = settings.dictation.dictionary;
  const add = async () => {
    const t = term.trim();
    if (!t || terms.includes(t)) return;
    setErr("");
    try {
      await set("dictation.dictionary", [...terms, t]);
      setTerm("");
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  return (
    <div className="mt-6">
      <Section title={`Personal dictionary (${terms.length}/200)`}>
        <div className="py-3">
          <p className="text-[12px] text-[var(--muted)] mb-2">Names and jargon that dictation should spell your way.</p>
          <div className="flex gap-2">
            <Input aria-label="New term" className="flex-1" maxLength={64} value={term} placeholder="e.g. Akhil, Tauri, Kubernetes" onChange={(e) => setTerm(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
            <Button onClick={add} disabled={!term.trim() || terms.length >= 200}>
              <Plus size={13} /> Add
            </Button>
          </div>
          <div className="flex flex-wrap gap-1.5 mt-3">
            {terms.map((t) => (
              <span key={t} className="inline-flex items-center gap-1 rounded-full bg-[var(--panel-2)] pl-2.5 pr-1 h-6 text-[12px]">
                {t}
                <button type="button" aria-label={`Remove ${t}`} className="h-5 w-5 inline-flex items-center justify-center rounded-full hover:bg-[var(--border)]" onClick={() => set("dictation.dictionary", terms.filter((x) => x !== t))}>
                  <X size={11} />
                </button>
              </span>
            ))}
          </div>
          <ErrorText>{err}</ErrorText>
        </div>
      </Section>
    </div>
  );
}

function Privacy() {
  const { settings, set, refresh } = useMain();
  const [confirm, setConfirm] = useState(false);
  const [removeKey, setRemoveKey] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!settings) return null;
  return (
    <>
      <Section title="What leaves your PC">
        <p className="py-3 text-[13px] leading-relaxed">{PRIVACY_TEXT}</p>
        <p className="pb-3 text-[12px] text-[var(--muted)] leading-relaxed">
          Wisp assumes the Gemini free tier. Enabling billing in Google AI Studio changes how Google may use your data; check Google's terms for the paid tier.
        </p>
      </Section>
      <Section title="History">
        <Row label="Keep dictation history" hint="Last 50 dictations for 7 days. Turning this off deletes it.">
          <Toggle label="Keep dictation history" checked={settings.dictation.history_enabled} onChange={(v) => set("dictation.history_enabled", v)} />
        </Row>
        <Row label="Clipboard history in the island" hint="Last 20 text items, in memory only, cleared on quit.">
          <Toggle label="Clipboard history" checked={settings.clipboard.history_enabled} onChange={(v) => set("clipboard.history_enabled", v)} />
        </Row>
        <Row label="Debug screenshots" hint="Saves Computer Use screenshots to %APPDATA%\Wisp\debug and deletes them after 24 hours. Off by default.">
          <Toggle label="Debug screenshots" checked={settings.agent.debug_screenshots} onChange={(v) => set("agent.debug_screenshots", v)} />
        </Row>
      </Section>
      <Section title="Delete">
        <Row label="Delete all data" hint="Wipes the database and logs on this PC. Your key is kept unless you choose to remove it.">
          <Button variant="danger" onClick={() => setConfirm(true)}>
            Delete all data…
          </Button>
        </Row>
      </Section>
      {confirm && (
        <Dialog
          title="Delete all data?"
          onClose={() => setConfirm(false)}
          actions={
            <>
              <Button variant="ghost" onClick={() => setConfirm(false)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                busy={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await ipc.deleteAllData(removeKey);
                    await refresh();
                  } finally {
                    setBusy(false);
                    setConfirm(false);
                  }
                }}
              >
                Delete everything
              </Button>
            </>
          }
        >
          <p>This removes to-dos, routines, history, presets, settings and logs. It cannot be undone.</p>
          <label className="flex items-center gap-2 mt-4 cursor-pointer">
            <input type="checkbox" className="h-4 w-4 accent-[var(--danger)]" checked={removeKey} onChange={(e) => setRemoveKey(e.target.checked)} />
            Also remove my Gemini key from Credential Manager
          </label>
        </Dialog>
      )}
    </>
  );
}

/** FR-CU-08 guard list (main window only, NFR-16), grace window, limits. */
function Safety() {
  const { settings, set } = useMain();
  const [proc, setProc] = useState("");
  const [word, setWord] = useState("");
  const [err, setErr] = useState("");
  if (!settings) return null;
  const g = settings.agent.guard_list;
  const save = async (next: typeof g) => {
    setErr("");
    try {
      await set("agent.guard_list", next);
    } catch (e) {
      setErr(errorMessage(e));
    }
  };
  const chips = (items: string[], remove: (x: string) => void) => (
    <div className="flex flex-wrap gap-1.5 mt-2">
      {items.map((x) => (
        <span key={x} className="inline-flex items-center gap-1 rounded-full bg-[var(--panel-2)] pl-2.5 pr-1 h-6 text-[12px]">
          {x}
          <button type="button" aria-label={`Remove ${x}`} className="h-5 w-5 inline-flex items-center justify-center rounded-full hover:bg-[var(--border)]" onClick={() => remove(x)}>
            <X size={11} />
          </button>
        </span>
      ))}
    </div>
  );
  return (
    <>
      <Section title="Guard list">
        <div className="py-3">
          <p className="text-[12px] text-[var(--muted)] mb-3">Computer Use pauses for your OK whenever one of these is in front, whatever the model says.</p>
          <div className="text-[13px] font-medium">Apps (process names)</div>
          <div className="flex gap-2 mt-1.5">
            <Input aria-label="Process name" className="flex-1" placeholder="Bitwarden.exe" value={proc} onChange={(e) => setProc(e.target.value)} />
            <Button disabled={!proc.trim()} onClick={() => { save({ ...g, processes: [...g.processes, proc.trim()] }); setProc(""); }}>
              Add
            </Button>
          </div>
          {chips(g.processes, (x) => save({ ...g, processes: g.processes.filter((p) => p !== x) }))}
          <div className="text-[13px] font-medium mt-4">Window titles containing</div>
          <div className="flex gap-2 mt-1.5">
            <Input aria-label="Title keyword" className="flex-1" placeholder="bank" value={word} onChange={(e) => setWord(e.target.value)} />
            <Button disabled={!word.trim()} onClick={() => { save({ ...g, title_keywords: [...g.title_keywords, word.trim()] }); setWord(""); }}>
              Add
            </Button>
          </div>
          {chips(g.title_keywords, (x) => save({ ...g, title_keywords: g.title_keywords.filter((p) => p !== x) }))}
          <ErrorText>{err}</ErrorText>
        </div>
      </Section>
      <Section title="Computer Use limits">
        <Row label="Monitor to control">
          <MonitorSelect label="Computer Use monitor" value={settings.capture.target_monitor} onChange={(v) => set("capture.target_monitor", v)} />
        </Row>
        <Row label="Max steps per task" hint="1 to 40">
          <Input type="number" aria-label="Max steps" className="w-[80px]" min={1} max={40} value={settings.agent.max_steps} onChange={(e) => set("agent.max_steps", Number(e.target.value))} />
        </Row>
        <Row label="Max minutes per task" hint="1 to 10">
          <Input type="number" aria-label="Max minutes" className="w-[80px]" min={1} max={10} value={settings.agent.max_minutes} onChange={(e) => set("agent.max_minutes", Number(e.target.value))} />
        </Row>
      </Section>
      <Section title="Routines">
        <Row label="Missed-run grace window" hint="If Wisp was closed or the PC was locked, runs this late still happen. Older ones are recorded as missed.">
          <Input type="number" aria-label="Grace minutes" className="w-[80px]" min={0} max={240} value={settings.scheduler.grace_minutes} onChange={(e) => set("scheduler.grace_minutes", Number(e.target.value))} />
          <span className="text-[13px] text-[var(--muted)]">min</span>
        </Row>
      </Section>
    </>
  );
}

function About() {
  const { status, refresh } = useMain();
  const [replacing, setReplacing] = useState(false);
  const [copied, setCopied] = useState(false);
  if (!status) return null;
  return (
    <>
      <Section title="Gemini key">
        <Row label="Key" hint="Stored in Windows Credential Manager as Wisp/GeminiApiKey.">
          <span className="font-mono text-[13px] text-[var(--muted)]">{status.key_masked ?? "none"}</span>
          <Button onClick={() => setReplacing((r) => !r)}>{replacing ? "Cancel" : "Replace"}</Button>
          {status.has_key && (
            <Button
              variant="danger"
              onClick={async () => {
                await ipc.secretsDeleteKey();
                await refresh();
              }}
            >
              Delete
            </Button>
          )}
        </Row>
        {replacing && (
          <div className="py-3">
            <KeyStep onSaved={() => setReplacing(false)} />
          </div>
        )}
        <Row label="Usage today" hint="Counts only. No content is stored.">
          <span className="text-[13px] text-[var(--muted)]">
            {status.requests_today} requests · {status.rate_limited_today} rate limited (HTTP 429)
          </span>
        </Row>
      </Section>
      <Section title="About">
        <Row label="Version">
          <span className="text-[13px] text-[var(--muted)]">Wisp {status.version}</span>
        </Row>
        <Row label="Diagnostics" hint="Version, Windows build, model IDs and counters. No content or key.">
          <Button
            onClick={async () => {
              await ipc.diagnosticsCopy();
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            }}
          >
            <ClipboardCopy size={13} /> {copied ? "Copied" : "Copy diagnostics"}
          </Button>
        </Row>
        <Row label="Logs" hint="%LOCALAPPDATA%\Wisp\logs, 7 days kept. Events and error codes only.">
          <Button onClick={() => ipc.openExternal("logs")}>
            <FolderOpen size={13} /> Open folder
          </Button>
        </Row>
        <Row label="Setup wizard">
          <Button onClick={() => useMain.getState().setPage("setup")}>Run again</Button>
        </Row>
        <Row label="Quit Wisp" hint="Closing this window only hides it.">
          <Button variant="danger" onClick={() => ipc.quitApp()}>
            Quit
          </Button>
        </Row>
      </Section>
    </>
  );
}
