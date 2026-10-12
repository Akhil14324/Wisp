# Wisp

A personal voice assistant for Windows 11, powered by your own Gemini API key. Hold a key and speak: Wisp types for you, answers questions about what is on screen, opens apps, runs routines, or operates the desktop itself. It lives in the tray and stores everything locally.

The full requirements are in [`docs/SRS.pdf`](docs/SRS.pdf). Where the build differs from them, see [`docs/DEVIATIONS.md`](docs/DEVIATIONS.md).

## Hotkeys

| Hold | Does |
|---|---|
| **Ctrl + Win** | Dictation: speak, release, and clean text lands where you are typing |
| **Ctrl + Shift** | Circle to Ask: draw around something on screen and ask about it |
| **Ctrl + Alt + Space** | Command mode: "open Spotify", "search spaghetti on YouTube", "add gym at 6 PM", "every morning at 9 open Unity" |
| **Esc ×3** (within 1.5 s) | Emergency stop for any running task |

Click the island pill to type a command instead. Hover it to see Music, Plans, Clipboard, Apps, Wallpapers and Quick settings. Chords can be changed in Settings > Hotkeys.

## One-time setup (Windows 11)

- Visual Studio Build Tools 2022 with **Desktop development with C++** (includes the Windows 11 SDK)
- Rust via `rustup`, toolchain `stable-x86_64-pc-windows-msvc`
- Node.js 20 or newer with npm
- Git
- WebView2 Runtime (already on Windows 11)

## Run, test, build

```powershell
npm install
npm run tauri dev      # run with hot reload
npm run tauri build    # NSIS installer in src-tauri/target/release/bundle/nsis/
```

Quality gates (also run in CI, `.github/workflows/build.yml`):

```powershell
npm run lint; npm run typecheck; npm test
cd src-tauri; cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test
```

The installer is per-user and unsigned, so there is no admin prompt. Windows SmartScreen or Defender may warn because Wisp combines a keyboard hook, input injection and screen capture. Allow it once, and if needed add an exclusion for its install folder (`%LOCALAPPDATA%\Wisp`). Updating means installing the next build over the old one; your data is kept.

## Where things live

| What | Where |
|---|---|
| Database (to-dos, routines, history, settings, presets) | `%APPDATA%\Wisp\wisp.db` |
| Logs (events and error codes only, 7 days) | `%LOCALAPPDATA%\Wisp\logs\` |
| Debug screenshots (off by default, deleted after 24 h) | `%APPDATA%\Wisp\debug\` |
| Gemini API key | Windows Credential Manager, `Wisp/GeminiApiKey` |

Audio, screenshots and clipboard text are sent to Google's Gemini API only for the request you make. On the free tier Google may use them to improve its products. Wisp sends nothing else anywhere: no telemetry, analytics or update checks.

## Models

Each role (dictation, commands, vision, Computer Use) is a setting in Settings > Models, filled from your key's live model list. Defaults: `gemini-3.5-flash-lite` for dictation and `gemini-3.8-flash` for the rest. Gemini Computer Use is a Preview feature and may change.

## Project layout

```
src/                 React front end, one Vite entry per window
  island/ overlay/ card/ main/ shared/
src-tauri/           Rust core (Tauri 2)
  src/hotkeys/       low-level keyboard hook and chord state machine
  src/gemini/        REST client, prompts, tool schema, Computer Use
  src/agent/         tool loop, confirmations, Computer Use loop
  src/scheduler/     routines, missed runs, to-do reminders
  src/store/         SQLite (refinery migrations in src-tauri/migrations)
  tests/             integration tests against a mock Gemini server
docs/manual-tests/   one checklist per milestone (M0 to M7)
```
