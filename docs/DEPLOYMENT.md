# Deployment notes

Wisp is a **Windows desktop app**. It ships as an NSIS installer built by `npm run tauri build` or by the GitHub Actions workflow. Everything that matters (hotkeys, audio, capture, input, Gemini calls, the database) runs in the Rust core on your PC.

## GitHub Actions

`.github/workflows/build.yml` runs on every push and pull request once the repo is on GitHub. It lints, typechecks and tests both halves, then uploads the unsigned installer as the `wisp-installer` artifact for 14 days.

```powershell
gh repo create wisp --private --source . --push
```

## Vercel and Railway (`vercel.json`, `railway.json`)

These configs are here so the projects can be connected later. They deploy **only the static front end** (`vite build` into `dist/`), as a UI preview:

- No Rust core runs there, so every button that calls Tauri fails, and the pages show little beyond their layout.
- No API key or user data is ever sent there.
- Wisp itself never contacts these hosts (NFR-14).

Connecting:

- Vercel: `vercel link`, then `vercel deploy`. `/` is rewritten to the main window page.
- Railway: create a service from the repo; `railway.json` builds the front end and serves `dist/` on `$PORT`.

If you later want a real web presence (for example a download page for the installer), add it as a separate site rather than serving the app's windows.
