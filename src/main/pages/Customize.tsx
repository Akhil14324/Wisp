import { Check, Copy, Download, Lock, Trash2, Upload } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useReload } from "../../shared/events";
import { errorMessage, ipc } from "../../shared/ipc";
import type { KeysPreset, Loadout, Preset, Skin, Slot, Soul } from "../../shared/types";
import { ALL_TOOLS, TOOL_LABELS } from "../../shared/types";
import { Badge, Button, ErrorText, Input, Select } from "../../shared/ui";
import { PageTitle } from "../App";
import { ALLOWED_CHORDS, ENGINES } from "./Settings";

const SLOTS: { id: Slot; label: string; blurb: string }[] = [
  { id: "soul", label: "Soul", blurb: "Name, tone, language and extra instructions. Never changes dictation." },
  { id: "loadout", label: "Loadout", blurb: "Which tools the assistant may use." },
  { id: "skin", label: "Skin", blurb: "Island and window theme." },
  { id: "keys", label: "Keys", blurb: "Hotkeys, default search engine, microphone and target monitor." },
];

/** Customization slots (section 4.10). */
export function CustomizePage() {
  const [slot, setSlot] = useState<Slot>("soul");
  const [presets, setPresets] = useState<Preset[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [err, setErr] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);
  useReload(["presets-changed"], () => {
    ipc.presetsList().then(setPresets).catch(() => {});
  });

  const list = presets.filter((p) => p.slot === slot);
  const current = list.find((p) => p.id === selected) ?? list.find((p) => p.is_active) ?? list[0];

  useEffect(() => setSelected(null), [slot]);

  const guard = async (f: () => Promise<unknown>) => {
    setErr("");
    try {
      await f();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  const duplicate = () =>
    guard(async () => {
      if (!current) return;
      let name = `${current.name} copy`;
      let n = 2;
      while (list.some((p) => p.name === name)) name = `${current.name} copy ${n++}`;
      const p = await ipc.presetsSave(slot, name, current.data);
      setSelected(p.id);
    });

  const exportPreset = () =>
    guard(async () => {
      if (!current) return;
      const text = await ipc.presetsExport(current.id);
      const blob = new Blob([text], { type: "application/json" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `wisp-${current.slot}-${current.name.replace(/[^\w-]+/g, "_")}.json`;
      a.click();
      URL.revokeObjectURL(a.href);
    });

  const importPreset = (file: File) =>
    guard(async () => {
      const text = await file.text();
      const p = await ipc.presetsImport(text);
      setSlot(p.slot);
      setSelected(p.id);
    });

  return (
    <>
      <PageTitle title="Customize" sub="Make Wisp your own. Changes apply right away." />
      <div role="tablist" aria-label="Slots" className="flex gap-1 mb-5">
        {SLOTS.map((s) => (
          <button
            key={s.id}
            role="tab"
            type="button"
            aria-selected={slot === s.id}
            onClick={() => setSlot(s.id)}
            className={`h-9 px-4 rounded-[var(--radius)] ${slot === s.id ? "bg-[var(--panel-2)] font-medium" : "text-[var(--muted)] hover:text-[var(--fg)]"}`}
          >
            {s.label}
          </button>
        ))}
      </div>
      <p className="text-[13px] text-[var(--muted)] mb-4">{SLOTS.find((s) => s.id === slot)?.blurb}</p>
      <div className="grid grid-cols-[220px_1fr] gap-5">
        <div className="flex flex-col gap-1">
          {list.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => setSelected(p.id)}
              aria-current={current?.id === p.id}
              className={`flex items-center gap-2 h-9 px-3 rounded-[var(--radius)] border text-left ${
                current?.id === p.id ? "border-[var(--accent)] bg-[var(--panel)]" : "border-transparent hover:bg-[var(--panel-2)]"
              }`}
            >
              <span className="flex-1 truncate">{p.name}</span>
              {p.is_builtin && <Lock size={12} className="text-[var(--muted)]" aria-label="Built-in" />}
              {p.is_active && <Badge tone="accent">Active</Badge>}
            </button>
          ))}
          <div className="flex flex-wrap gap-1.5 mt-3">
            <Button onClick={duplicate}>
              <Copy size={13} /> Duplicate
            </Button>
            <Button onClick={() => fileRef.current?.click()}>
              <Upload size={13} /> Import
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept=".json,application/json"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) importPreset(f);
                e.target.value = "";
              }}
            />
          </div>
        </div>
        {current && (
          <div className="rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] p-5">
            <PresetEditor key={current.id} preset={current} onError={setErr} />
            <div className="flex flex-wrap gap-2 mt-5 pt-4 border-t border-[var(--border)]">
              {!current.is_active && (
                <Button variant="primary" onClick={() => guard(() => ipc.presetsActivate(current.id))}>
                  <Check size={13} /> Use this preset
                </Button>
              )}
              <Button onClick={exportPreset}>
                <Download size={13} /> Export
              </Button>
              {!current.is_builtin && (
                <Button variant="danger" onClick={() => guard(() => ipc.presetsDelete(current.id))}>
                  <Trash2 size={13} /> Delete
                </Button>
              )}
            </div>
            <ErrorText>{err}</ErrorText>
          </div>
        )}
      </div>
    </>
  );
}

function PresetEditor({ preset, onError }: { preset: Preset; onError: (e: string) => void }) {
  const [name, setName] = useState(preset.name);
  const [data, setData] = useState<unknown>(preset.data);
  const [saved, setSaved] = useState(false);
  const readOnly = preset.is_builtin;
  const dirty = useMemo(() => name !== preset.name || JSON.stringify(data) !== JSON.stringify(preset.data), [name, data, preset]);

  const save = async () => {
    onError("");
    try {
      await ipc.presetsSave(preset.slot, name.trim(), data, preset.id);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1200);
    } catch (e) {
      onError(errorMessage(e));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      {readOnly ? (
        <div className="text-[12px] text-[var(--muted)] flex items-center gap-1.5">
          <Lock size={12} /> Built-in preset. Duplicate it to make changes.
        </div>
      ) : (
        <Field label="Preset name">
          <Input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} />
        </Field>
      )}
      <fieldset disabled={readOnly} className="flex flex-col gap-3 disabled:opacity-80">
        {preset.slot === "soul" && <SoulEditor value={data as Soul} onChange={setData} />}
        {preset.slot === "loadout" && <LoadoutEditor value={data as Loadout} onChange={setData} />}
        {preset.slot === "skin" && <SkinEditor value={data as Skin} onChange={setData} />}
        {preset.slot === "keys" && <KeysEditor value={data as KeysPreset} onChange={setData} />}
      </fieldset>
      {!readOnly && (
        <div>
          <Button variant="primary" disabled={!dirty} onClick={save}>
            {saved ? "Saved" : "Save changes"}
          </Button>
        </div>
      )}
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[12px] text-[var(--muted)]">{label}</span>
      {children}
      {hint && <span className="text-[11px] text-[var(--muted)]">{hint}</span>}
    </label>
  );
}

function SoulEditor({ value, onChange }: { value: Soul; onChange: (v: Soul) => void }) {
  return (
    <>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Assistant name">
          <Input value={value.name} maxLength={40} onChange={(e) => onChange({ ...value, name: e.target.value })} />
        </Field>
        <Field label="Reply language">
          <Input value={value.language} maxLength={40} onChange={(e) => onChange({ ...value, language: e.target.value })} />
        </Field>
      </div>
      <Field label="Tone">
        <Input value={value.tone} maxLength={100} onChange={(e) => onChange({ ...value, tone: e.target.value })} />
      </Field>
      <Field label="Extra instructions" hint={`${value.addendum.length}/500. Added to command and answer prompts.`}>
        <textarea
          className="min-h-[90px] rounded-[var(--radius)] border border-[var(--border)] bg-[var(--panel)] p-2.5 text-[13px] outline-none focus:border-[var(--accent)]"
          maxLength={500}
          value={value.addendum}
          onChange={(e) => onChange({ ...value, addendum: e.target.value })}
        />
      </Field>
    </>
  );
}

function LoadoutEditor({ value, onChange }: { value: Loadout; onChange: (v: Loadout) => void }) {
  return (
    <div className="grid grid-cols-2 gap-x-4 gap-y-2">
      {ALL_TOOLS.map((t) => {
        const on = value.tools.includes(t);
        return (
          <label key={t} className="flex items-center gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              className="h-4 w-4 accent-[var(--accent)]"
              checked={on}
              onChange={() => onChange({ tools: on ? value.tools.filter((x) => x !== t) : [...value.tools, t] })}
            />
            {TOOL_LABELS[t]}
            {t === "run_computer_task" && <Badge tone="warn">mouse + keyboard</Badge>}
          </label>
        );
      })}
    </div>
  );
}

function SkinEditor({ value, onChange }: { value: Skin; onChange: (v: Skin) => void }) {
  return (
    <>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Theme">
          <Select value={value.theme} onChange={(e) => onChange({ ...value, theme: e.target.value as Skin["theme"] })}>
            <option value="auto">Match Windows</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </Select>
        </Field>
        <Field label="Accent colour">
          <div className="flex items-center gap-2">
            <input type="color" aria-label="Accent colour" className="h-8 w-10 rounded border border-[var(--border)] bg-transparent" value={value.accent} onChange={(e) => onChange({ ...value, accent: e.target.value })} />
            <span className="text-[12px] text-[var(--muted)] font-mono">{value.accent}</span>
          </div>
        </Field>
      </div>
      <Field label={`Corner radius: ${value.radius}px`}>
        <input type="range" min={0} max={24} value={value.radius} onChange={(e) => onChange({ ...value, radius: Number(e.target.value) })} />
      </Field>
      <Field label={`Island opacity: ${Math.round(value.opacity * 100)}%`}>
        <input type="range" min={30} max={100} value={Math.round(value.opacity * 100)} onChange={(e) => onChange({ ...value, opacity: Number(e.target.value) / 100 })} />
      </Field>
      <Field label={`Island font size: ${value.font_size}px`}>
        <input type="range" min={11} max={18} value={value.font_size} onChange={(e) => onChange({ ...value, font_size: Number(e.target.value) })} />
      </Field>
    </>
  );
}

function KeysEditor({ value, onChange }: { value: KeysPreset; onChange: (v: KeysPreset) => void }) {
  const chordSelect = (k: "dictation" | "circle" | "command", label: string) => (
    <Field label={label}>
      <Select value={value[k]} onChange={(e) => onChange({ ...value, [k]: e.target.value })}>
        {ALLOWED_CHORDS.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
      </Select>
    </Field>
  );
  const clash = new Set([value.dictation, value.circle, value.command]).size < 3;
  return (
    <>
      <div className="grid grid-cols-3 gap-3">
        {chordSelect("dictation", "Dictation")}
        {chordSelect("circle", "Circle to Ask")}
        {chordSelect("command", "Command mode")}
      </div>
      {clash && <div className="text-[12px] text-[var(--danger)]">Each action needs a different chord.</div>}
      <div className="grid grid-cols-2 gap-3">
        <Field label="Default search engine">
          <Select value={value.search_engine} onChange={(e) => onChange({ ...value, search_engine: e.target.value })}>
            {ENGINES.map((e) => (
              <option key={e} value={e}>
                {e}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Computer Use monitor" hint='"primary" or a monitor id from Settings'>
          <Input value={value.target_monitor} onChange={(e) => onChange({ ...value, target_monitor: e.target.value })} />
        </Field>
      </div>
    </>
  );
}
