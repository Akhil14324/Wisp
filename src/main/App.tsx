import { CalendarClock, History, Home, ListTodo, Palette, Settings as SettingsIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useEvent, useReload } from "../shared/events";
import { useSkin } from "../shared/theme";
import { CustomizePage } from "./pages/Customize";
import { HistoryPage } from "./pages/History";
import { HomePage } from "./pages/Home";
import { RoutinesPage } from "./pages/Routines";
import { SettingsPage } from "./pages/Settings";
import { SetupWizard } from "./pages/Setup";
import { TodoPage } from "./pages/Todo";
import { type Page, useMain } from "./store";

const NAV: { id: Page; label: string; icon: ReactNode }[] = [
  { id: "home", label: "Home", icon: <Home size={17} /> },
  { id: "todo", label: "To-do", icon: <ListTodo size={17} /> },
  { id: "routines", label: "Routines", icon: <CalendarClock size={17} /> },
  { id: "history", label: "History", icon: <History size={17} /> },
  { id: "customize", label: "Customize", icon: <Palette size={17} /> },
  { id: "settings", label: "Settings", icon: <SettingsIcon size={17} /> },
];

export function App() {
  useSkin();
  const { page, setPage, status, refresh } = useMain();
  useReload(["settings-changed", "presets-changed"], () => {
    refresh().catch(() => {});
  });
  useEvent<{ page: string }>("navigate", (p) => {
    if (NAV.some((n) => n.id === p.page) || p.page === "setup") setPage(p.page as Page);
  });

  if (!status) return <div className="h-screen bg-[var(--bg)]" />;
  const needsSetup = !status.first_run_done || !status.has_key;
  if (needsSetup || page === "setup") {
    return (
      <div className="h-screen bg-[var(--bg)] text-[var(--fg)] flex items-center justify-center">
        <SetupWizard onDone={() => setPage("home")} />
      </div>
    );
  }

  return (
    <div className="h-screen flex bg-[var(--bg)] text-[var(--fg)] text-[14px]">
      <nav aria-label="Pages" className="w-[200px] shrink-0 border-r border-[var(--border)] p-3 flex flex-col gap-0.5">
        <div className="flex items-center gap-2 px-2.5 pt-1 pb-4">
          <WispMark />
          <span className="font-semibold text-[15px]">Wisp</span>
        </div>
        {NAV.map((n) => (
          <button
            key={n.id}
            type="button"
            aria-current={page === n.id ? "page" : undefined}
            onClick={() => setPage(n.id)}
            className={`flex items-center gap-2.5 h-9 px-2.5 rounded-[var(--radius)] text-left transition ${
              page === n.id ? "bg-[var(--panel-2)] text-[var(--fg)] font-medium" : "text-[var(--muted)] hover:text-[var(--fg)] hover:bg-[var(--panel-2)]"
            }`}
          >
            {n.icon}
            {n.label}
          </button>
        ))}
        <div className="mt-auto px-2.5 text-[11px] text-[var(--muted)]">
          {status.hotkeys_paused ? "Hotkeys paused" : "Hold Ctrl + Win to dictate"}
        </div>
      </nav>
      <main className="flex-1 min-w-0 overflow-y-auto">
        <div className="max-w-[860px] mx-auto px-8 py-7">
          {page === "home" && <HomePage />}
          {page === "todo" && <TodoPage />}
          {page === "routines" && <RoutinesPage />}
          {page === "history" && <HistoryPage />}
          {page === "customize" && <CustomizePage />}
          {page === "settings" && <SettingsPage />}
        </div>
      </main>
    </div>
  );
}

export function WispMark({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden>
      <defs>
        <radialGradient id="wisp-g" cx="50%" cy="62%" r="55%">
          <stop offset="0%" stopColor="#ffffff" />
          <stop offset="45%" stopColor="var(--accent)" />
          <stop offset="100%" stopColor="var(--accent)" stopOpacity="0" />
        </radialGradient>
      </defs>
      <path d="M16 3c3 5 9 8 9 15a9 9 0 0 1-18 0c0-4 2-6 4-8 0 3 1 5 3 6-1-5 0-9 2-13z" fill="url(#wisp-g)" />
    </svg>
  );
}

export function PageTitle({ title, sub, actions }: { title: string; sub?: string; actions?: ReactNode }) {
  return (
    <div className="flex items-end justify-between mb-6 gap-4">
      <div>
        <h1 className="text-[22px] font-semibold">{title}</h1>
        {sub && <p className="text-[13px] text-[var(--muted)] mt-1">{sub}</p>}
      </div>
      {actions}
    </div>
  );
}
