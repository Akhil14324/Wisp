# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Wisp: a Windows 11-only voice assistant (Tauri 2 + Rust core + React 18 front end) built from `docs/SRS.pdf`. Requirement IDs (`FR-DIC-06`, `NFR-16`, …) are cited in code comments; keep citing them. Anything built differently from the SRS must be recorded in `docs/DEVIATIONS.md`.

## Commands

```powershell
npm run tauri dev                 # run the app (Vite on :1420 + Rust)
npm run tauri build               # NSIS installer (per-user, unsigned)
npm run lint; npm run typecheck; npm test          # front end (ESLint, tsc strict, Vitest)
npx vitest run src/shared/format.test.ts           # one front-end test file

cd src-tauri
cargo test                        # unit + integration tests (tests/gemini_mock.rs uses wiremock)
cargo test --lib hotkeys::chord   # one module's tests
cargo test --test gemini_mock tool_loop   # matching integration tests
cargo clippy --all-targets -- -D warnings; cargo fmt --check
```

CI (`.github/workflows/build.yml`) runs all of the above on `windows-latest`. The island, overlay and card are excluded from screen capture; for screenshots of them in dev builds, start with `$env:WISP_SHOW_IN_CAPTURE = "1"`. `$env:WISP_LOG = "debug"` raises log verbosity. Disk space on the dev PC is tight: `[profile.dev]` uses minimal debug info on purpose, and `cargo clean` is safe if `target/` grows.

## Architecture

The UI never holds the API key and never calls Gemini. It invokes Tauri commands and listens to `wisp://<topic>` events. Everything that listens, captures, injects input or calls Google is in Rust.

- **Global state**: `state::ctx()` returns a `&'static AppState` (set once in `lib.rs::setup`). Async code uses it directly instead of passing `Arc`s around.
- **Event flow**: `hotkeys` (dedicated thread, `WH_KEYBOARD_LL` + `WH_MOUSE_LL`, pure state machine in `hotkeys/chord.rs`) sends `HookEvent`s over a channel. `dispatch.rs` routes them: Ctrl+Win to `voice` + `dictation`, Ctrl+Alt+Space to `voice` + `agent::run_command`, Ctrl+Shift to `cta` (Circle to Ask). `sysevents.rs` (a hidden window) feeds lock/unlock, resume and clipboard events the same way.
- **Agent**: `agent/tool_loop.rs` is the generic generateContent function-calling loop (8 rounds, stops after 2 consecutive tool failures), generic over a `ToolHost` trait so tests can script it. `tools/exec.rs::SystemTools` is the real host; `tools/mod.rs` holds the tool names and the pure validators. `agent/computer.rs` is the Computer Use loop with safety decisions, the guard list, stuck detection and takeover pause. `AgentControl` owns cancellation (emergency stop = Esc×3) and confirmation cards.
- **Gemini wire formats**: dictation, commands and vision use REST `generateContent` (`gemini/client.rs`, retries 429/5xx with 1/2/4 s + Retry-After). Computer Use uses the **Interactions API**, all of it isolated in `gemini/computer_use.rs`. The rule from the SRS: never invent request shapes. Read the official docs page before changing either.
- **Model turns** are appended to history unchanged (`gemini::model_content`) so thought signatures round-trip.
- **Storage**: one SQLite file via `store::Db` (a mutex-wrapped connection; `refinery` migrations in `src-tauri/migrations/`). Settings are dotted keys over the Appendix A defaults tree (`store/settings.rs`: `KEYS` allowlist plus `validate`). Side effects of a setting change live in `commands::apply_setting`. Presets (Soul/Loadout/Skin/Keys) are in `store/presets.rs`; the active Loadout decides which tools are declared to the model.
- **To-dos have two backends**: `todolist.rs` routes every to-do call to TaskHub (`taskhub.rs`, the user's own Express API, signed in from Settings > General; token and password in Credential Manager) or, when signed out, to the local `store/todos.rs`. Commands, the `create_todo` tool and reminders must go through `todolist`, never `store::todos` directly. The TaskHub API shapes are documented at the top of `taskhub.rs`, from `Desktop/taskhub-mobile/vgrand-taskhub-backend`.
- **Island window** has a fixed size (`island::WINDOW`). A hit-test thread makes everything outside the visible shape click-through, and the shape morphs in CSS (`.island-morph`, `src/island/shape.ts`). `island_layout` reports the shape size; it does not resize the window.
- **Scheduler** (`scheduler/mod.rs`): one loop computes due jobs, to-do reminders and the 03:00 retention pass, and wakes at least every 60 s. Runs go through a bounded queue (5) to a single worker that calls `agent::run_command` unattended.
- **Windows**: `island` (pill/panel, `WS_EX_NOACTIVATE`; the front end asks Rust to resize via `island_layout`), `overlay` (pre-created, drawing or click-through rings), `card` (Circle to Ask answer), `main` (settings, history, wizard). Island, overlay and card are excluded from capture. Each window has its own Vite entry under `src/<window>/` and its own capability file in `src-tauri/capabilities/`.

## Adding a Tauri command

The command must be added in **four places**, or calls fail with a permission error:

1. the function in `src-tauri/src/commands.rs`
2. `generate_handler!` in `lib.rs`
3. the `COMMANDS` list in `src-tauri/build.rs`
4. the `allow-<kebab-name>` permission in each window's capability JSON that may call it

Then add a typed wrapper in `src/shared/ipc.ts`. JS argument names are camelCase versions of the Rust parameter names. Commands that change safety-relevant state (`agent.guard_list`, `allow_computer_use`, the key, delete-all) must call `main_only(&window)`. Model tools must never reach them (NFR-16).

## Rules from the SRS that shape the code

- The API key lives only in Credential Manager (`secrets.rs`; `ApiKey`'s Debug/Display redact it). Never log prompts, transcripts, audio, screenshots or clipboard text. Logs hold events and codes only.
- Model output is data: validate every tool argument (`tools::str_arg`, `validate_url` with http/https only, the key allowlist in `input/keys.rs`, the 0-999 coordinate checks).
- No network traffic except Gemini and URLs the user asked to open.
- Hook callbacks must stay allocation-light and lock-free (FR-HK-04); state lives in `thread_local`s on the hook thread.
- Hardware features can't be unit-tested. Each milestone has a manual checklist in `docs/manual-tests/M<n>.md`; update the matching one when behaviour changes.
- Files written from PowerShell 5.1 get a UTF-8 BOM, which breaks JSON parsing. Use the Write tool, or strip the BOM.
