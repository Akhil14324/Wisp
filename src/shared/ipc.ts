// Typed wrappers over Tauri commands (section 3.6). The UI never holds the key
// and never calls Gemini; it only sends commands and receives events.
import { invoke } from "@tauri-apps/api/core";
import type {
  AppEntry,
  AppStatus,
  ClipItem,
  CommandEntry,
  DictationEntry,
  Job,
  JobRun,
  KeyCheck,
  KeyStatus,
  MediaInfo,
  ModelsList,
  MonitorInfo,
  Pinned,
  Preset,
  Settings,
  Slot,
  TaskHubStatus,
  Stroke,
  Todo,
  UiError,
  Wallpaper,
  When,
} from "./types";

export function isUiError(e: unknown): e is UiError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

/** Human-readable message for any thrown value. */
export function errorMessage(e: unknown): string {
  if (isUiError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);

export const ipc = {
  appStatus: () => call<AppStatus>("app_status"),

  secretsStatus: () => call<KeyStatus>("secrets_status"),
  secretsSetKey: (key: string) => call<KeyStatus>("secrets_set_key", { key }),
  secretsDeleteKey: () => call<KeyStatus>("secrets_delete_key"),
  validateKey: (key?: string) => call<KeyCheck>("validate_key", { key: key ?? null }),
  listModels: () => call<ModelsList>("list_models"),

  settingsGet: () => call<Settings>("settings_get"),
  settingsSet: (key: string, value: unknown) => call<Settings>("settings_set", { key, value }),

  presetsList: () => call<Preset[]>("presets_list"),
  presetsSave: (slot: Slot, name: string, data: unknown, id?: number) =>
    call<Preset>("presets_save", { id: id ?? null, slot, name, data }),
  presetsActivate: (id: number) => call<Preset>("presets_activate", { id }),
  presetsDelete: (id: number) => call<void>("presets_delete", { id }),
  presetsExport: (id: number) => call<string>("presets_export", { id }),
  presetsImport: (text: string) => call<Preset>("presets_import", { text }),

  todosList: () => call<Todo[]>("todos_list"),
  todosCreate: (title: string, due_at: string | null, notes: string | null = null) =>
    call<Todo>("todos_create", { todo: { title, due_at, notes } }),
  todosUpdate: (patch: { id: number; title?: string; notes?: string | null; due_at?: string | null; done?: boolean }) =>
    call<Todo | null>("todos_update", { patch }),
  todosDelete: (id: number) => call<void>("todos_delete", { id }),
  todosReorder: (ids: number[]) => call<void>("todos_reorder", { ids }),
  todoSnooze: (id: number, minutes = 10) => call<void>("todo_snooze", { id, minutes }),

  taskhubStatus: () => call<TaskHubStatus>("taskhub_status"),
  taskhubSignIn: (url: string, username: string, password: string) =>
    call<TaskHubStatus>("taskhub_sign_in", { url, username, password }),
  taskhubSignOut: () => call<TaskHubStatus>("taskhub_sign_out"),

  jobsList: () => call<Job[]>("jobs_list"),
  jobsCreate: (job: { name: string; command: string; when: When; allow_computer_use: boolean; enabled?: boolean }) =>
    call<Job>("jobs_create", { job }),
  jobsUpdate: (id: number, job: { name: string; command: string; when: When; allow_computer_use: boolean; enabled: boolean }) =>
    call<Job>("jobs_update", { id, job }),
  jobsDelete: (id: number) => call<void>("jobs_delete", { id }),
  jobsRunNow: (id: number) => call<void>("jobs_run_now", { id }),
  jobRunsList: (jobId?: number) => call<JobRun[]>("job_runs_list", { jobId: jobId ?? null }),

  runTextCommand: (text: string) => call<{ text: string; ok: boolean }>("run_text_command", { text }),
  runAgentTask: (goal: string) => call<{ text: string; ok: boolean }>("run_agent_task", { goal }),
  agentStop: () => call<void>("agent_stop"),
  agentResume: () => call<void>("agent_resume"),
  confirmAnswer: (id: number, yes: boolean) => call<boolean>("confirm_answer", { id, yes }),

  historyCommands: () => call<CommandEntry[]>("history_commands"),
  historyDictations: () => call<DictationEntry[]>("history_dictations"),
  copyDictation: (id: number) => call<void>("copy_dictation", { id }),

  appsSearch: (query: string) => call<AppEntry[]>("apps_search", { query }),
  appsPinned: () => call<Pinned[]>("apps_pinned"),
  appsPin: (appId: string) => call<void>("apps_pin", { appId }),
  appsUnpin: (appId: string) => call<void>("apps_unpin", { appId }),
  launchApp: (appId: string) => call<void>("launch_app", { appId }),
  mediaControl: (action: "play_pause" | "next" | "previous") => call<void>("media_control", { action }),
  mediaGet: () => call<MediaInfo>("media_get"),
  volumeGet: () => call<{ level: number; muted: boolean }>("volume_get"),
  volumeSetMute: (muted: boolean) => call<void>("volume_set_mute", { muted }),
  wallpaperList: () => call<Wallpaper[]>("wallpaper_list"),
  wallpaperSet: (path: string) => call<void>("wallpaper_set", { path }),
  wallpaperPickFolder: (folder: string) => call<void>("wallpaper_pick_folder", { folder }),
  clipboardHistoryList: () => call<ClipItem[]>("clipboard_history_list"),
  clipboardHistoryPaste: (id: number) => call<void>("clipboard_history_paste", { id }),

  micDevicesList: () => call<string[]>("mic_devices_list"),
  micTest: () => call<{ ok: boolean; peak: number; message: string }>("mic_test"),
  monitorsList: () => call<MonitorInfo[]>("monitors_list"),
  windowShowMain: (page?: string) => call<void>("window_show_main", { page: page ?? null }),
  deleteAllData: (removeKey: boolean) => call<void>("delete_all_data", { removeKey }),
  diagnosticsCopy: () => call<string>("diagnostics_copy"),
  hotkeysSetPaused: (paused: boolean) => call<void>("hotkeys_set_paused", { paused }),
  openExternal: (target: "aistudio" | "mic_privacy" | "typing_settings" | "logs") => call<void>("open_external", { target }),
  quitApp: () => call<void>("quit_app"),

  islandLayout: (width: number, height: number) => call<void>("island_layout", { width, height }),
  islandSetInteractive: (on: boolean) => call<void>("island_set_interactive", { on }),
  overlaySubmit: (strokes: Stroke[]) => call<void>("overlay_submit", { strokes }),
  cardResize: (height: number) => call<void>("card_resize", { height }),
  ctaAsk: (text: string) => call<string>("cta_ask", { text }),
  ctaClose: () => call<void>("cta_close"),
};
