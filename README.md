# Offline AI Chat

**A portable, truly offline personal AI assistant — a native Windows app, powered by [llama.cpp](https://github.com/ggml-org/llama.cpp), that runs entirely from a drive you control. No internet, no account, no data ever leaving your machine, and nothing left behind on the computer you ran it on.**

Build it once, then carry it (and your models) on a pendrive. It's a real desktop app window — not a browser tab — with its own chat history store, offline document search, and two privacy modes: keep conversations saved to the drive, or make them disappear the moment they end.

![platform](https://img.shields.io/badge/platform-Windows-blue) ![license](https://img.shields.io/badge/license-MIT-green) ![status](https://img.shields.io/badge/status-early--stage-orange)

### Contents

[Why offline AI](#why) · [Quick start](#quick-start) · [Choosing a model](#choosing-a-model) · [Privacy modes](#privacy-modes) · [How it works](#how-it-works) · [Project structure](#project-structure) · [Troubleshooting](#troubleshooting) · [FAQ](#faq) · [Roadmap](#roadmap)

## Why

Most "AI chat" tools ship your prompts to someone else's server. This one doesn't:

- **100% offline** — once a model is on the drive, no internet connection is required to chat, search your documents, or do anything else.
- **Nothing lands on the host system** — every path the app touches (chat history, document index, cache) is resolved relative to wherever the app's `.exe` itself is sitting, never `%AppData%`/the registry/the host's local disk.
- **Your choice of privacy mode, every conversation** — *Normal* mode saves chat history to the drive only; *Disappearing* mode keeps it in memory and explicitly wipes it the moment the conversation ends.
- **Multi-model, controlled from the terminal** — list, load, or run several `.gguf` models at once with `--list-models` / `--model <name>` (repeatable).
- **Offline document search** — drop in `.txt`, `.md`, `.pdf`, `.docx`, or `.xlsx`/`.xls`/`.ods` files and the app answers questions using them as context, entirely locally (keyword search via SQLite FTS5). Attach a document right from the chat, or manage the whole library from the sidebar.
- **Live model management from the UI** — see every model on the drive, load or unload any of them while the app is running (no restart, no CLI needed), with per-model status and per-reply latency (time-to-first-token, tokens/sec).
- **Drive-removal fail-safe** — the app detects the drive disappearing mid-session, stops cleanly, and recovers automatically from a damaged database file on next launch rather than corrupting silently.
- **OpenAI-compatible local API** — `llama-server` exposes a `/v1/chat/completions` endpoint on `127.0.0.1`, so you can point other local tools at it too.

## Quick start

**Prerequisites (to build):** Windows 10/11, [Rust](https://rustup.rs/) + Cargo, [Node.js](https://nodejs.org/) + npm. (WebView2 is pre-installed on Windows 10/11; no separate browser needed to *run* the built app.)

1. **Clone this repo and build the app:**
   ```bash
   git clone https://github.com/182004jaideep/offline-ai-chat.git
   cd offline-ai-chat/app
   npm install
   npm run tauri build
   ```
   The built executable lands under `app/src-tauri/target/release/`.
2. **Copy the built `.exe` to the repo root** (next to `models/` and `engine/`) — this is what makes it a self-contained, portable layout you can carry on a drive.
3. **Run `setup.bat`** (from the repo root) if `engine/llama-server.exe` and its DLLs aren't already present — it fetches the llama.cpp engine (MIT-licensed, from the [official releases page](https://github.com/ggml-org/llama.cpp/releases)) into `engine/` and tells you exactly which model to grab next.
4. **Download a model** (one-time) and drop the `.gguf` file into `models/`. See [Choosing a model](#choosing-a-model).
5. **Run the app.** Double-click it, or from a terminal: `--list-models` to see what's available, `--model <name>` to load a specific one (repeatable for more than one at once). With no flags it loads the first model it finds.
6. **Pick a privacy mode** in the window that opens, then chat. Closing the window stops the engine and cleans up automatically.

## Choosing a model

Any [GGUF](https://huggingface.co/docs/hub/gguf)-format model works. With no `--model` flag, the app loads the first `.gguf` file it finds in `models/`. Some starting points:

| Use case            | Model                                                                                             | Approx. size | RAM needed |
|----------------------|-----------------------------------------------------------------------------------------------------|:------------:|:----------:|
| Low-end machines     | [Qwen2.5-Coder-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF)    | ~1 GB        | 8 GB       |
| Balanced / coding     | [Qwen2.5-Coder-7B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF)        | ~4.5 GB (Q4_K_M) | 16 GB |
| General-purpose chat | [Meta-Llama-3.1-8B-Instruct-GGUF](https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF) | ~4.9 GB (Q4_K_M) | 16 GB |

Pick a `Q4_K_M` quantization unless you know you want something else. The app warns (but doesn't block) if your selected model(s) need more RAM than is currently available.

## Privacy modes

Chosen fresh for every new conversation, from a modal shown on launch:

- **Normal — saved to this drive.** Chat history is written to a SQLite database at `data/offlineai.db`, resolved relative to the app's own location — never to the host computer. Past conversations are listed in the sidebar and can be reopened or deleted at any time.
- **Disappearing — memory only.** Nothing is written to disk, not even on the drive. Messages live only in the app's memory and are explicitly overwritten (not just dropped) the instant the conversation ends. This can't make a forensic guarantee against the *host OS's* own memory paging or hibernation — only against anything this app itself would otherwise write.

## How it works

```
app.exe (Tauri: Rust backend + native window, no browser chrome)
   │
   ├─ Resolves every path (models/, data/, cache) relative to its own
   │   location, never %AppData%/the registry
   ├─ Parses CLI flags: --model (repeatable), --list-models,
   │   --ingest / --list-documents / --search (document index)
   ├─ Spawns one llama-server.exe per selected model, each on its own
   │   port (127.0.0.1:8585, 8586, ...), OpenAI-compatible API
   ├─ Opens data/offlineai.db (SQLite, WAL mode) -- integrity-checked on
   │   open; a damaged file is quarantined and replaced automatically
   ├─ Polls whether the drive is still present; on loss, stops the
   │   engine, wipes any open disappearing session, and warns the UI
   └─ On window close: kills every engine process, wipes ephemeral
       sessions, no process or file handle survives
```

The frontend (`app/src/index.html`) is a single-file Tauri webview: a sidebar for past chats and ingested documents, a chat view with lightweight markdown rendering, and a privacy-mode picker — talking to the local engine over `fetch()` and to the Rust backend over Tauri's IPC.

## Project structure

```
offline-ai-chat/
├── app/                  # Tauri project (Rust backend + webview frontend)
│   ├── src/               # index.html -- the chat UI
│   └── src-tauri/          # Rust: models, db (SQLite), ephemeral (disappearing mode),
│                           #       watchdog (drive-loss detection)
├── OfflineAI.exe           # The built app -- stays at the root so you can double-click it
├── setup.bat              # One-time helper: downloads the llama.cpp engine into engine/
├── engine/                 # llama-server.exe + its DLLs (gitignored, fetched by setup.bat)
│   └── LICENSE-llama.cpp.txt  # License text for the bundled llama.cpp binaries
├── models/                 # Put your .gguf model file(s) here (gitignored)
├── data/                    # Created at runtime: SQLite chat/document store (gitignored)
├── LICENSE
└── README.md
```

`engine/`'s contents, every `.gguf` model, and the `data/` database are intentionally **not** committed to this repo (see `.gitignore`) — `setup.bat` fetches the engine, the model download is manual since the "right" model depends on your hardware, and `data/` is the user's own private chat history.

## Troubleshooting

- **"No .gguf model found"** — make sure the model file is directly inside `models/`, not a subfolder, and that it has a `.gguf` extension. Run with `--list-models` to see what the app actually finds.
- **Port 8585 (or later) already in use** — the app automatically tries the next port for each additional model; if a single model still fails to load, check what else is bound to `127.0.0.1:8585`.
- **"Database looked damaged" warning** — this happens if the drive was unplugged mid-write in a previous session. The app automatically moves the damaged file aside (`data/offlineai.*.corrupt-*.db`) and starts fresh; your old data isn't deleted, just renamed.
- **"Drive disconnected" banner** — the drive the app is running from became unreachable. Reconnect it and restart the app; there's no automatic resume once this happens.
- **High RAM-usage warning at launch** — the selected model(s)' combined size is close to or over your currently available RAM. The app still tries to load them, but may swap heavily or fail; pick a smaller model or load fewer at once.

## FAQ

**Is this really free?**
Yes. The project is MIT-licensed, llama.cpp is MIT-licensed, and the models linked above are free to download. You only need hardware capable of running them — no subscription, no API key, no account.

**Does it actually work with no internet connection?**
Yes, once `setup.bat` has fetched the engine and you've downloaded a `.gguf` model. From that point on the app never makes a network call — `llama-server`, the chat UI, and the database all run entirely on-device.

**Is my data really private with offline AI?**
Nothing you type is sent anywhere — there's no server to send it to. The chat UI talks only to `127.0.0.1` on the same machine. In Normal mode, history is written only to the drive the app runs from; in Disappearing mode, nothing is written at all.

**How is this different from ChatGPT or other cloud AI assistants?**
Cloud assistants run the model on someone else's servers and your prompts travel over the internet to get there. Offline AI Chat runs the same kind of model (an open-weight LLM via llama.cpp) locally on your CPU/RAM — slower per token than a data-center GPU, but nothing ever leaves your device, there's no account, and it keeps working with no internet at all.

**What hardware do I need to run an LLM locally?**
A reasonably modern CPU and 8–16 GB of RAM covers the models listed in [Choosing a model](#choosing-a-model). A GPU isn't required — this ships with a CPU-only engine by default, though llama.cpp also publishes CUDA/Vulkan builds if you want to accelerate it manually.

**Does it work on macOS or Linux?**
The app is built on Tauri, which is cross-platform in principle, but this repo currently only builds and is tested for Windows (the bundled engine binaries, `setup.bat`, and the drive-loss watchdog are all Windows-specific right now). Cross-platform support is on the [Roadmap](#roadmap).

## Roadmap

- [ ] Packaged, one-command release build (today you build from source with Rust + Node)
- [ ] macOS / Linux build targets
- [ ] UI for the notes/tracked-data feature (schema and backend commands already exist)
- [ ] GPU-accelerated engine option (CUDA/Vulkan builds are already published upstream by llama.cpp)

See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## Credits

- [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT License) — the inference engine this project wraps. All model execution, quantization, and the OpenAI-compatible API are its work, not this repo's.
- [Tauri](https://tauri.app/) (MIT/Apache-2.0) — the native app shell (Rust backend + OS webview).
- Model weights are the property of their respective creators (Qwen, Meta, etc.) under their own licenses — check each model card before use.

## License

MIT — see [LICENSE](LICENSE).
