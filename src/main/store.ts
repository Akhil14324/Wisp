// Main window view state.
import { create } from "zustand";
import { ipc } from "../shared/ipc";
import type { AppStatus, Settings } from "../shared/types";

export type Page = "home" | "todo" | "routines" | "history" | "customize" | "settings" | "setup";

interface MainStore {
  page: Page;
  status: AppStatus | null;
  settings: Settings | null;
  setPage: (p: Page) => void;
  refresh: () => Promise<void>;
  set: (key: string, value: unknown) => Promise<void>;
}

export const useMain = create<MainStore>((setState) => ({
  page: "home",
  status: null,
  settings: null,
  setPage: (page) => setState({ page }),
  refresh: async () => {
    const [status, settings] = await Promise.all([ipc.appStatus(), ipc.settingsGet()]);
    setState({ status, settings });
  },
  set: async (key, value) => {
    const settings = await ipc.settingsSet(key, value);
    setState({ settings });
  },
}));
