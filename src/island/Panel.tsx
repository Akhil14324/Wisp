import { Check, ExternalLink, Music2, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useState } from "react";
import { useEvent, useReload } from "../shared/events";
import { errorMessage, ipc } from "../shared/ipc";
import { filterTodos, mmss, todoDueLabel, truncate, whenLabel } from "../shared/format";
import type { ClipItem, Job, MediaInfo, Pinned, Preset, Settings, Todo, Wallpaper } from "../shared/types";
import { TABS, useIsland } from "./store";

/** Expanded panel with six tabs (FR-ISL-03). Scrolling cycles tabs. */
export function Panel() {
  const { tab, setTab } = useIsland();
  return (
    <div className="flex flex-col" style={{ height: 300 }}>
      <div role="tablist" aria-label="Island panels" className="flex gap-0.5 px-2 pt-2 pb-1.5 text-[12px] overflow-hidden">
        {TABS.map((t) => (
          <button key={t} role="tab" type="button" aria-selected={tab === t} className="island-tab" onClick={() => setTab(t)}>
            {t === "Quick settings" ? "Quick" : t}
          </button>
        ))}
        <button type="button" className="island-tab ml-auto" aria-label="Open Wisp" title="Open Wisp" onClick={() => ipc.windowShowMain()}>
          <ExternalLink size={13} />
        </button>
      </div>
      <div role="tabpanel" className="flex-1 min-h-0 px-3 pb-3">
        {tab === "Music" && <MusicTab />}
        {tab === "Plans" && <PlansTab />}
        {tab === "Clipboard" && <ClipboardTab />}
        {tab === "Apps" && <AppsTab />}
        {tab === "Wallpapers" && <WallpapersTab />}
        {tab === "Quick settings" && <QuickTab />}
      </div>
    </div>
  );
}

/** FR-ISL-05: title, artist, artwork, controls and progress, updated by event. */
function MusicTab() {
  const [m, setM] = useState<MediaInfo | null>(null);
  const [now, setNow] = useState(Date.now());
  const [anchor, setAnchor] = useState(Date.now());
  useEffect(() => {
    ipc.mediaGet().then((x) => {
      setM(x);
      setAnchor(Date.now());
    }).catch(() => {});
    const t = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(t);
  }, []);
  useEvent<MediaInfo>("media-changed", (x) => {
    setM(x);
    setAnchor(Date.now());
  });
  if (!m) return <div className="muted text-center pt-16">Loading…</div>;
  if (!m.has_session) return <div className="muted text-center pt-16">Nothing is playing</div>;
  const pos = Math.min(m.duration_ms || 0, m.position_ms + (m.playing ? now - anchor : 0));
  const pct = m.duration_ms > 0 ? (pos / m.duration_ms) * 100 : 0;
  const act = (a: "play_pause" | "next" | "previous") => ipc.mediaControl(a).catch(() => {});
  return (
    <div className="flex flex-col h-full justify-center gap-4 px-1">
      <div className="flex items-center gap-3.5">
        {m.artwork ? (
          <img src={m.artwork} alt="" className="h-20 w-20 rounded-lg object-cover shrink-0" />
        ) : (
          <div className="h-20 w-20 rounded-lg bg-white/10 flex items-center justify-center shrink-0">
            <Music2 size={26} className="muted" />
          </div>
        )}
        <div className="min-w-0">
          <div className="font-semibold text-[15px] truncate">{m.title || "Unknown title"}</div>
          <div className="muted truncate">{m.artist}</div>
          {m.album && <div className="muted truncate text-[12px]">{m.album}</div>}
        </div>
      </div>
      {m.duration_ms > 0 && (
        <div>
          <div className="h-1 rounded-full bg-white/10 overflow-hidden" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
            <div className="h-full bg-[var(--accent)]" style={{ width: `${pct}%` }} />
          </div>
          <div className="flex justify-between muted text-[11px] mt-1">
            <span>{mmss(pos)}</span>
            <span>{mmss(m.duration_ms)}</span>
          </div>
        </div>
      )}
      <div className="flex items-center justify-center gap-3">
        <button type="button" className="island-icon-btn" aria-label="Previous" onClick={() => act("previous")}>
          <SkipBack size={17} />
        </button>
        <button type="button" className="island-icon-btn h-10 w-10 bg-white/10" aria-label={m.playing ? "Pause" : "Play"} onClick={() => act("play_pause")}>
          {m.playing ? <Pause size={18} /> : <Play size={18} />}
        </button>
        <button type="button" className="island-icon-btn" aria-label="Next" onClick={() => act("next")}>
          <SkipForward size={17} />
        </button>
      </div>
    </div>
  );
}

/** FR-ISL-06: today's to-dos with a count and the next 3 jobs. */
function PlansTab() {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  useReload(["todos-changed", "jobs-changed"], () => {
    ipc.todosList().then(setTodos).catch(() => {});
    ipc.jobsList().then(setJobs).catch(() => {});
  });
  const today = filterTodos(todos, "today");
  const doneToday = todos.filter((t) => t.done && t.completed_at && new Date(t.completed_at).toDateString() === new Date().toDateString());
  const total = today.length + doneToday.length;
  const next = jobs
    .filter((j) => j.enabled && j.next_run_at)
    .sort((a, b) => (a.next_run_at! < b.next_run_at! ? -1 : 1))
    .slice(0, 3);
  return (
    <div className="island-scroll h-full pr-1">
      <div className="flex items-baseline justify-between px-1 mb-1">
        <span className="font-semibold">Today</span>
        <span className="muted text-[12px]">
          {doneToday.length}/{total}
        </span>
      </div>
      {today.length === 0 && <div className="muted px-1 py-2 text-[12px]">Nothing left for today.</div>}
      {today.map((t) => (
        <label key={t.id} className="island-row cursor-pointer">
          <input
            type="checkbox"
            className="accent-[var(--accent)] h-3.5 w-3.5"
            checked={t.done}
            onChange={() => ipc.todosUpdate({ id: t.id, done: !t.done })}
          />
          <span className="truncate flex-1">{t.title}</span>
          {t.due_at && <span className="muted text-[11px] shrink-0">{todoDueLabel(t)}</span>}
        </label>
      ))}
      <div className="font-semibold px-1 mt-3 mb-1">Next up</div>
      {next.length === 0 && <div className="muted px-1 text-[12px]">No scheduled jobs.</div>}
      {next.map((j) => (
        <div key={j.id} className="island-row">
          <span className="truncate flex-1">{j.name}</span>
          <span className="muted text-[11px] shrink-0">{whenLabel(j.next_run_at)}</span>
        </div>
      ))}
    </div>
  );
}

/** FR-ISL-07: last 20 text items, memory only, off by default. */
function ClipboardTab() {
  const [items, setItems] = useState<ClipItem[]>([]);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useReload(["clipboard-changed", "settings-changed"], () => {
    ipc.settingsGet().then((s) => setEnabled(s.clipboard.history_enabled)).catch(() => {});
    ipc.clipboardHistoryList().then(setItems).catch(() => {});
  });
  if (enabled === false)
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-center">
        <div className="muted text-[12px] max-w-[280px]">Clipboard history is off. Items are kept in memory only and cleared when Wisp quits.</div>
        <button type="button" className="island-btn" onClick={() => ipc.settingsSet("clipboard.history_enabled", true)}>
          Turn on
        </button>
      </div>
    );
  if (items.length === 0) return <div className="muted text-center pt-16 text-[12px]">Copy some text and it shows up here.</div>;
  return (
    <div className="island-scroll h-full pr-1">
      {items.map((it) => (
        <button key={it.id} type="button" className="island-row w-full text-left" onClick={() => ipc.clipboardHistoryPaste(it.id).catch(() => {})}>
          <span className="truncate">{truncate(it.text.replace(/\s+/g, " "), 120)}</span>
        </button>
      ))}
    </div>
  );
}

/** FR-ISL-08: up to 12 pinned apps. */
function AppsTab() {
  const [pins, setPins] = useState<Pinned[]>([]);
  useReload(["pins-changed"], () => {
    ipc.appsPinned().then(setPins).catch(() => {});
  });
  if (pins.length === 0)
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <div className="muted text-[12px]">No pinned apps yet.</div>
        <button type="button" className="island-btn" onClick={() => ipc.windowShowMain("settings")}>
          Pin apps in Settings
        </button>
      </div>
    );
  return (
    <div className="grid grid-cols-4 gap-2 pt-1">
      {pins.map((p) => (
        <button key={p.app_id} type="button" className="flex flex-col items-center gap-1.5 p-2 rounded-xl hover:bg-white/10" onClick={() => ipc.launchApp(p.app_id).catch(() => {})}>
          <span className="h-9 w-9 rounded-xl bg-white/10 flex items-center justify-center text-[15px] font-semibold">{p.label.charAt(0).toUpperCase()}</span>
          <span className="text-[11px] w-full truncate text-center">{p.label}</span>
        </button>
      ))}
    </div>
  );
}

/** FR-ISL-09: thumbnails from the chosen folder. */
function WallpapersTab() {
  const [list, setList] = useState<Wallpaper[] | null>(null);
  const [err, setErr] = useState("");
  useReload(["settings-changed"], () => {
    ipc.wallpaperList().then((l) => {
      setList(l);
      setErr("");
    }).catch((e) => setErr(errorMessage(e)));
  });
  if (err)
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <div className="muted text-[12px] text-center max-w-[280px]">{err}</div>
        <button type="button" className="island-btn" onClick={() => ipc.windowShowMain("settings")}>
          Open Settings
        </button>
      </div>
    );
  if (!list) return <div className="muted text-center pt-16">Loading…</div>;
  if (list.length === 0) return <div className="muted text-center pt-16 text-[12px]">No images in the folder.</div>;
  return (
    <div className="island-scroll h-full grid grid-cols-3 gap-2 content-start pr-1">
      {list.map((w) => (
        <button key={w.path} type="button" title={w.name} className="rounded-lg overflow-hidden hover:ring-2 ring-[var(--accent)]" onClick={() => ipc.wallpaperSet(w.path).catch(() => {})}>
          <img src={w.thumb} alt={w.name} className="w-full h-[68px] object-cover" />
        </button>
      ))}
    </div>
  );
}

/** FR-ISL-10 and FR-CUS-02: pause hotkeys, microphone, mute, autostart, presets. */
function QuickTab() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [mics, setMics] = useState<string[]>([]);
  const [paused, setPaused] = useState(false);
  const [muted, setMuted] = useState(false);
  const [presets, setPresets] = useState<Preset[]>([]);
  useReload(["settings-changed", "presets-changed"], () => {
    ipc.settingsGet().then(setSettings).catch(() => {});
    ipc.appStatus().then((s) => setPaused(s.hotkeys_paused)).catch(() => {});
    ipc.presetsList().then(setPresets).catch(() => {});
  });
  useEffect(() => {
    ipc.micDevicesList().then(setMics).catch(() => {});
    ipc.volumeGet().then((v) => setMuted(v.muted)).catch(() => {});
  }, []);
  if (!settings) return null;
  const slotSelect = (slot: "soul" | "loadout" | "skin", label: string) => {
    const opts = presets.filter((p) => p.slot === slot);
    const active = opts.find((p) => p.is_active);
    return (
      <label className="flex items-center justify-between gap-2">
        <span className="muted">{label}</span>
        <select aria-label={label} value={active?.id ?? ""} onChange={(e) => ipc.presetsActivate(Number(e.target.value))} className="w-[150px]">
          {opts.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
    );
  };
  return (
    <div className="island-scroll h-full flex flex-col gap-2.5 pr-1 pt-1">
      <QuickToggle label="Pause hotkeys" on={paused} onChange={(v) => { setPaused(v); ipc.hotkeysSetPaused(v); }} />
      <QuickToggle label="Mute speakers" on={muted} onChange={(v) => { setMuted(v); ipc.volumeSetMute(v); }} />
      <QuickToggle label="Start with Windows" on={settings.app.autostart} onChange={(v) => ipc.settingsSet("app.autostart", v)} />
      <label className="flex items-center justify-between gap-2">
        <span className="muted">Microphone</span>
        <select
          aria-label="Microphone"
          className="w-[150px]"
          value={settings.audio.input_device ?? ""}
          onChange={(e) => ipc.settingsSet("audio.input_device", e.target.value || null)}
        >
          <option value="">System default</option>
          {mics.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </label>
      {slotSelect("soul", "Soul")}
      {slotSelect("loadout", "Loadout")}
      {slotSelect("skin", "Skin")}
    </div>
  );
}

function QuickToggle({ label, on, onChange }: { label: string; on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button type="button" role="switch" aria-checked={on} className="flex items-center justify-between w-full" onClick={() => onChange(!on)}>
      <span className="muted">{label}</span>
      <span className={`relative h-5 w-9 rounded-full transition ${on ? "bg-[var(--accent)]" : "bg-white/15"}`}>
        <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition ${on ? "left-[18px]" : "left-0.5"}`}>
          {on && <Check size={10} className="m-[3px] text-[var(--accent)]" />}
        </span>
      </span>
    </button>
  );
}
