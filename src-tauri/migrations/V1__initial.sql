CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

CREATE TABLE presets (
  id         INTEGER PRIMARY KEY,
  slot       TEXT NOT NULL CHECK (slot IN ('soul','loadout','skin','keys')),
  name       TEXT NOT NULL,
  json       TEXT NOT NULL,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  is_active  INTEGER NOT NULL DEFAULT 0,
  UNIQUE (slot, name)
);

CREATE TABLE todos (
  id            INTEGER PRIMARY KEY,
  title         TEXT NOT NULL,
  notes         TEXT,
  due_at        TEXT,
  done          INTEGER NOT NULL DEFAULT 0,
  sort_order    INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  completed_at  TEXT,
  snoozed_until TEXT
);

CREATE TABLE jobs (
  id                 INTEGER PRIMARY KEY,
  name               TEXT NOT NULL,
  command            TEXT NOT NULL,
  when_json          TEXT NOT NULL,
  enabled            INTEGER NOT NULL DEFAULT 1,
  allow_computer_use INTEGER NOT NULL DEFAULT 0,
  next_run_at        TEXT,
  created_at         TEXT NOT NULL
);

CREATE TABLE job_runs (
  id          INTEGER PRIMARY KEY,
  job_id      INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  due_at      TEXT NOT NULL,
  started_at  TEXT,
  status      TEXT NOT NULL CHECK (status IN ('ok','failed','missed','aborted')),
  summary     TEXT,
  duration_ms INTEGER
);

CREATE TABLE command_history (
  id          INTEGER PRIMARY KEY,
  at          TEXT NOT NULL,
  source      TEXT NOT NULL CHECK (source IN ('voice','typed','job','agent_task')),
  transcript  TEXT,
  tools_json  TEXT,
  outcome     TEXT NOT NULL,
  duration_ms INTEGER
);

CREATE TABLE dictation_history (
  id   INTEGER PRIMARY KEY,
  at   TEXT NOT NULL,
  text TEXT NOT NULL
);

CREATE TABLE agent_steps (
  id          INTEGER PRIMARY KEY,
  history_id  INTEGER NOT NULL REFERENCES command_history(id) ON DELETE CASCADE,
  at          TEXT NOT NULL,
  step        INTEGER NOT NULL,
  intent      TEXT,
  action_json TEXT NOT NULL,
  safety      TEXT,
  user_answer TEXT
);

CREATE TABLE pinned_apps (
  id       INTEGER PRIMARY KEY,
  app_id   TEXT NOT NULL UNIQUE,
  label    TEXT NOT NULL,
  position INTEGER NOT NULL
);

CREATE TABLE usage_counters (
  day    TEXT NOT NULL,
  metric TEXT NOT NULL,
  value  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, metric)
);
