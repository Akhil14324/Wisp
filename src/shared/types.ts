// Types mirroring the Rust IPC contract (Appendix B).

export type ErrCode =
  | "NO_KEY"
  | "KEY_INVALID"
  | "RATE_LIMIT"
  | "NETWORK"
  | "MODEL_NOT_FOUND"
  | "SAFETY_BLOCKED"
  | "TIMEOUT"
  | "EMPTY_RESULT"
  | "INTERNAL"
  | "INVALID"
  | "FORBIDDEN"
  | "CANCELLED";

export interface UiError {
  code: ErrCode;
  message: string;
}

export type IslandStateName = "idle" | "listening" | "thinking" | "acting" | "confirm" | "result" | "error";

export interface IslandEvent {
  state: IslandStateName;
  text?: string;
  code?: ErrCode;
  controlling: boolean;
}

export interface AppStatus {
  version: string;
  has_key: boolean;
  key_masked: string | null;
  first_run_done: boolean;
  privacy_ack_at: string | null;
  hotkeys_paused: boolean;
  requests_today: number;
  rate_limited_today: number;
  animations: boolean;
  live_available: boolean;
  db_recovered: boolean;
}

export interface KeyStatus {
  has_key: boolean;
  masked: string | null;
}

export interface KeyCheck {
  ok: boolean;
  model_count: number;
  status: number | null;
  cause: string | null;
}

export interface ModelInfo {
  id: string;
  display_name: string;
  methods: string[];
}

export interface ModelsList {
  models: ModelInfo[];
  missing: string[];
}

export interface GuardList {
  processes: string[];
  title_keywords: string[];
}

export interface Settings {
  schema_version: number;
  models: { dictation: string; command: string; vision: string; computer_use: string; live: string | null };
  hotkeys: { dictation: string; circle: string; command: string; emergency_stop: string };
  search: { default_engine: string };
  audio: { input_device: string | null };
  capture: { target_monitor: string };
  island: { monitor: string; hide_in_fullscreen: boolean };
  dictation: { dictionary: string[]; history_enabled: boolean };
  clipboard: { history_enabled: boolean };
  scheduler: { grace_minutes: number };
  agent: { guard_list: GuardList; debug_screenshots: boolean; max_steps: number; max_minutes: number };
  app: { autostart: boolean; first_run_done: boolean; privacy_ack_at: string | null };
  wallpaper: { folder: string | null };
}

export type Slot = "soul" | "loadout" | "skin" | "keys";

export interface Preset<T = unknown> {
  id: number;
  slot: Slot;
  name: string;
  data: T;
  is_builtin: boolean;
  is_active: boolean;
}

export interface Soul {
  name: string;
  tone: string;
  language: string;
  addendum: string;
}

export interface Loadout {
  tools: ToolName[];
}

export interface Skin {
  theme: "light" | "dark" | "auto";
  accent: string;
  radius: number;
  opacity: number;
  font_size: number;
}

export interface KeysPreset {
  dictation: string;
  circle: string;
  command: string;
  search_engine: string;
  input_device: string | null;
  target_monitor: string;
}

export type ToolName =
  | "open_app"
  | "open_url"
  | "web_search"
  | "type_text"
  | "press_keys"
  | "clipboard_read"
  | "clipboard_write"
  | "media_control"
  | "set_volume"
  | "set_wallpaper"
  | "describe_screen"
  | "create_schedule"
  | "create_routine"
  | "run_computer_task"
  | "create_todo";

export interface Todo {
  id: number;
  title: string;
  notes: string | null;
  due_at: string | null;
  done: boolean;
  sort_order: number;
  created_at: string;
  completed_at: string | null;
  snoozed_until: string | null;
}

export type WhenKind = "once" | "daily" | "weekdays" | "weekly" | "monthly" | "cron";

export interface When {
  kind: WhenKind;
  date?: string;
  time?: string;
  days?: string[];
  day_of_month?: number;
  cron?: string;
}

export interface Job {
  id: number;
  name: string;
  command: string;
  when: When;
  enabled: boolean;
  allow_computer_use: boolean;
  next_run_at: string | null;
  created_at: string;
}

export interface JobRun {
  id: number;
  job_id: number;
  job_name: string | null;
  due_at: string;
  started_at: string | null;
  status: "ok" | "failed" | "missed" | "aborted";
  summary: string | null;
  duration_ms: number | null;
}

export interface CallRecord {
  name: string;
  args: Record<string, unknown>;
  ok: boolean;
  error?: string;
}

export interface AgentStep {
  step: number;
  at: string;
  intent: string | null;
  action: Record<string, unknown>;
  safety: string | null;
  user_answer: string | null;
}

export interface CommandEntry {
  id: number;
  at: string;
  source: "voice" | "typed" | "job" | "agent_task";
  transcript: string | null;
  tools: CallRecord[] | null;
  outcome: string;
  duration_ms: number | null;
  steps: AgentStep[];
}

export interface DictationEntry {
  id: number;
  at: string;
  text: string;
}

export interface AppEntry {
  id: string;
  name: string;
  kind: "shortcut" | "store";
}

export interface Pinned {
  app_id: string;
  label: string;
  position: number;
}

export interface MediaInfo {
  has_session: boolean;
  title: string;
  artist: string;
  album: string;
  artwork: string | null;
  playing: boolean;
  position_ms: number;
  duration_ms: number;
  app: string;
}

export interface Wallpaper {
  path: string;
  name: string;
  thumb: string;
}

export interface ClipItem {
  id: number;
  text: string;
  at: string;
}

export interface MonitorInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  primary: boolean;
}

export type ConfirmKind = "first_task" | "safety" | "guard" | "takeover";

export interface ConfirmRequest {
  id: number;
  title: string;
  detail: string;
  kind: ConfirmKind;
}

export interface Stroke {
  points: [number, number][];
}

export interface CardUpdate {
  state: "loading" | "answer" | "error" | "ask";
  text: string;
  can_follow_up: boolean;
}

export const TOOL_LABELS: Record<ToolName, string> = {
  open_app: "Open apps",
  open_url: "Open web pages",
  web_search: "Web search",
  type_text: "Type text",
  press_keys: "Press keys",
  clipboard_read: "Read clipboard",
  clipboard_write: "Write clipboard",
  media_control: "Media controls",
  set_volume: "Volume",
  set_wallpaper: "Wallpaper",
  describe_screen: "Describe screen",
  create_schedule: "Schedule tasks",
  create_routine: "Create routines",
  run_computer_task: "Computer Use",
  create_todo: "To-dos",
};

export const ALL_TOOLS = Object.keys(TOOL_LABELS) as ToolName[];
