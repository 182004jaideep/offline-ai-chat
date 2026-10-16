# Contributing to Offline AI Chat

Thanks for considering a contribution! This project is intentionally small and dependency-free (one `.bat`, one `.html`, no build step), so it's a friendly place for a first PR.

## Ways to help

- **Cross-platform launchers** — a macOS/Linux equivalent of `Run-Office-AI.bat` is the single highest-value contribution right now. See the [Roadmap](README.md#roadmap) in the README.
- **UI improvements** — `index.html` is plain HTML/CSS/JS on purpose. Keep it dependency-free (no build tooling) so it keeps working as a static file with no setup.
- **Bug reports** — if `Run-Office-AI.bat` or `setup.bat` fails on your machine, please open an issue with your Windows version, the exact error text, and whether the project folder lives on an internal drive or removable media.
- **Docs** — clearer setup steps, more recommended models, troubleshooting notes from real failures you hit.

## Ground rules

- Keep it dependency-free. No npm/build step for the UI; no new runtime requirements for the launcher beyond what Windows ships with (`cmd`, PowerShell).
- Don't commit binaries or model weights. `setup.bat` and the README are the mechanism for getting those onto a user's machine — see `.gitignore`.
- Test the actual double-click flow before opening a PR, not just that the code looks right. This project lives or dies by "does it just work when someone double-clicks it."
- Small, focused PRs over big rewrites — easier to review, easier to merge.

## Development setup

There's no build step. Edit `Run-Office-AI.bat`, `setup.bat`, or `index.html` directly, then run `Run-Office-AI.bat` to test end-to-end (you'll need a `.gguf` model in `models/` — see the README).

## Reporting security issues

If you find something that could leak data outside `127.0.0.1` or defeat the "zero-trace" temp-profile cleanup, please open an issue describing the problem — this project's entire value proposition is staying local and private, so those reports get priority.
