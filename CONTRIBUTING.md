# Contributing to Offline AI Chat

Thanks for considering a contribution! The project is a Tauri app: a Rust backend (`app/src-tauri/`) plus a single-file HTML/CSS/JS frontend (`app/src/index.html`), wrapping the `llama.cpp` inference engine. No separate web framework, no bundler for the frontend — but a real build step via Cargo + npm.

## Ways to help

- **Cross-platform builds** — the app currently only builds and is tested for Windows (bundled engine binaries, `setup.bat`, and the drive-loss watchdog are all Windows-specific). A macOS/Linux build target is the single highest-value contribution right now. See the [Roadmap](README.md#roadmap).
- **More document formats** — `.txt`/`.md`/`.pdf`/`.docx`/`.xlsx` are supported now (see `app/src-tauri/src/db.rs::extract_text`); `.pptx` and other formats are still open.
- **UI** — `app/src/index.html` is plain HTML/CSS/JS on purpose, no frontend framework or bundler. Keep it that way; it keeps the app lightweight and avoids an extra build toolchain for the webview layer.
- **Notes/tracked-data UI** — the `notes` table and backend commands (`add_note`/`list_notes`/`delete_note`) already exist with no frontend surface for them yet.
- **Bug reports** — if the build, `setup.bat`, or the app itself fails on your machine, please open an issue with your Windows version, the exact error text, and whether the project folder lives on an internal drive or removable media.
- **Docs** — clearer setup steps, more recommended models, troubleshooting notes from real failures you hit.

## Ground rules

- **Nothing lands on the host system.** This is the project's entire reason to exist — every path the app touches must resolve relative to the app's own executable location (see `drive_root()` in `app/src-tauri/src/lib.rs`), never `%AppData%`/the registry/a hardcoded host path. Any change that writes outside the drive the app runs from is a bug, not a feature.
- **Don't weaken a privacy mode silently.** Disappearing-mode messages must never reach `db.rs`/SQLite; Normal-mode messages must never be assumed persisted if the drive watchdog has flagged it lost (see `check_drive` in `lib.rs`).
- **Don't commit binaries or model weights.** `setup.bat` and the README are the mechanism for getting those onto a user's machine — see `.gitignore`.
- **Keep the frontend dependency-free.** No npm packages, no bundler, no CDN scripts for `app/src/index.html` — it must keep working as a single static file served by Tauri with no build step of its own.
- **Test the actual built app before opening a PR**, not just that the code compiles. This project lives or dies by "does it actually work when run from a USB drive."
- **Small, focused PRs over big rewrites** — easier to review, easier to merge.

## Development setup

```bash
cd app
npm install
npm run tauri dev    # hot-reloads the frontend, rebuilds Rust on change
```

You'll need a `.gguf` model in `models/` at the repo root (not `app/`) — the dev build still resolves paths relative to wherever the running binary ends up, same as a release build. Rust-only changes can be checked faster with `cd app/src-tauri && cargo check` / `cargo test` before doing a full `tauri dev` run.

To test like a real portable deployment: `npm run tauri build`, then copy the resulting `.exe` out to the repo root (or another folder with `models/` + `engine/` containing `llama-server.exe` and its DLLs) and run it from there, not from inside `app/src-tauri/target/`.

## Reporting security issues

If you find something that could leak chat data or document content outside `127.0.0.1`, cause Normal-mode data to be written off the drive, cause Disappearing-mode data to be written anywhere at all, or defeat the drive-loss/corruption fail-safes, please open an issue describing the problem — this project's entire value proposition is staying local and private, so those reports get priority.
