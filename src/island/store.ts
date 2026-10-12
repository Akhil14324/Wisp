// Island view state (Zustand). View state only; decisions live in Rust.
import { create } from "zustand";
import type { AppEntry, ConfirmRequest, IslandEvent } from "../shared/types";

export type View = "collapsed" | "expanded" | "input";
export const TABS = ["Music", "Plans", "Clipboard", "Apps", "Wallpapers", "Quick settings"] as const;
export type Tab = (typeof TABS)[number];

export interface JobNotice {
  name: string;
  when: string;
}

interface IslandStore {
  ev: IslandEvent;
  level: number;
  levels: number[];
  confirm: ConfirmRequest | null;
  choices: AppEntry[];
  job: JobNotice | null;
  paused: boolean;
  view: View;
  tab: Tab;
  setEvent: (e: IslandEvent) => void;
  pushLevel: (rms: number) => void;
  setConfirm: (c: ConfirmRequest | null) => void;
  setChoices: (c: AppEntry[]) => void;
  setJob: (j: JobNotice | null) => void;
  setPaused: (p: boolean) => void;
  setView: (v: View) => void;
  setTab: (t: Tab) => void;
  cycleTab: (dir: 1 | -1) => void;
}

export const BARS = 9;

export const useIsland = create<IslandStore>((set, get) => ({
  ev: { state: "idle", controlling: false },
  level: 0,
  levels: Array(BARS).fill(0),
  confirm: null,
  choices: [],
  job: null,
  paused: false,
  view: "collapsed",
  tab: "Music",
  setEvent: (ev) => set({ ev, levels: ev.state === "listening" ? get().levels : Array(BARS).fill(0) }),
  pushLevel: (rms) => set((s) => ({ level: rms, levels: [...s.levels.slice(1), rms] })),
  setConfirm: (confirm) => set({ confirm }),
  setChoices: (choices) => set({ choices }),
  setJob: (job) => set({ job }),
  setPaused: (paused) => set({ paused }),
  setView: (view) => set({ view }),
  setTab: (tab) => set({ tab }),
  cycleTab: (dir) =>
    set((s) => {
      const i = TABS.indexOf(s.tab);
      return { tab: TABS[(i + dir + TABS.length) % TABS.length] };
    }),
}));
