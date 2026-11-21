# Offline AI Chat

**A portable, truly offline personal AI assistant — a native Windows app, powered by [llama.cpp](https://github.com/ggml-org/llama.cpp), that runs entirely from a drive you control. No internet, no account, no data ever leaving your machine, and nothing left behind on the computer you ran it on.**

Build it once, then carry it (and your models) on a pendrive. It's a real desktop app window — not a browser tab — with its own chat history store, offline document search, and two privacy modes: keep conversations saved to the drive, or make them disappear the moment they end.

![platform](https://img.shields.io/badge/platform-Windows-blue) ![license](https://img.shields.io/badge/license-MIT-green) ![status](https://img.shields.io/badge/status-early--stage-orange)

### Contents

[Why offline AI](#why) · [Licensing & who can use this](#licensing--who-can-use-this) · [Quick start](#quick-start) · [Choosing a model](#choosing-a-model) · [Limitations](#limitations) · [Privacy modes](#privacy-modes) · [How it works](#how-it-works) · [Project structure](#project-structure) · [Troubleshooting](#troubleshooting) · [FAQ](#faq) · [Roadmap](#roadmap) · [Contributing](#contributing)

## Why

Most "AI chat" tools ship your prompts to someone else's server. This one doesn't:

- **100% offline** — once a model is on the drive, no internet connection is required to chat, search your documents, or do anything else.
- **Nothing lands on the host system** — every path the app touches (chat history, document index, cache) is resolved relative to wherever the app's `.exe` itself is sitting, never `%AppData%`/the registry/the host's local disk.
- **Your choice of privacy mode, every conversation** — *Normal* mode saves chat history to the drive only; *Disappearing* mode keeps it in memory and explicitly wipes it the moment the conversation ends.
- **Multi-model, controlled from the terminal** — list, load, or run several `.gguf` models at once with `--list-models` / `--model <name>` (repeatable).
- **Offline document search** — drop in `.txt`, `.md`, `.pdf`, `.docx`, or `.xlsx`/`.xls`/`.ods` files and the app answers questions using them as context, entirely locally (keyword search via SQLite FTS5). Attach a document right from the chat, or manage the whole library from the sidebar.
- **Live model management from the UI** — see every model on the drive, load or unload any of them while the app is running (no restart, no CLI needed), with per-model status and per-reply latency (time-to-first-token, tokens/sec).
- **A dedicated Coding mode** — source-code-aware ingestion, syntax-highlighted code blocks with Copy/Save, and a system prompt that explicitly tells the model it cannot execute code or access your filesystem (so it won't claim it "ran" something it didn't).
- **Quick notes**, saved locally alongside your chat history — for anything you want to jot down without it being a conversation.
- **Drive-removal fail-safe** — the app detects the drive disappearing mid-session, stops cleanly, and recovers automatically from a damaged database file on next launch rather than corrupting silently.
- **OpenAI-compatible local API** — `llama-server` exposes a `/v1/chat/completions` endpoint on `127.0.0.1`, so you can point other local tools at it too.

## Licensing & who can use this

Short answer: **yes, you can use this, including commercially**, for most of the models linked in this README. Longer answer, because "AI license" questions usually have a catch hiding in the model weights rather than the code:

| Component | License | What it means for you |
|---|---|---|
| This app (`offline-ai-chat`) | [MIT](LICENSE) | Use, modify, redistribute, sell — no restrictions, no attribution requirement beyond keeping the license file. |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (the inference engine) | MIT | Same as above. Bundled as a compiled binary in `engine/`, not modified. |
| [Tauri](https://tauri.app/) (the app shell) | MIT / Apache-2.0 | Same as above. |
| Model weights (Qwen, Llama, etc.) | **Each model's own license — not this project's** | See below. Always check before shipping a product around one. |

**Model licenses vary and some have real strings attached:**
- **Qwen2.5 / Qwen2.5-Coder** models → Apache-2.0. Fully permissive, commercial use included, no special conditions.
- **Meta Llama 3.1 / 3.2** models → the [Llama Community License](https://www.llama.com/llama3_1/license/), not MIT/Apache. It's free for almost everyone, but it has an actual clause: if your product or service has **more than 700 million monthly active users**, you need a separate license from Meta. It also requires a small attribution notice ("Built with Llama") in some redistribution cases. Read the license page, not just this summary, before building a product on it.
- Any other GGUF model you drop in `models/` — check that model's own card on Hugging Face before commercial use. This project has no control over, and makes no claims about, third-party model licenses.

**Who this is for:** individuals, students, researchers, and companies alike. There's no account, no telemetry, no usage cap, and no field-of-use restriction from this project itself — the only thing to double-check is the license of whichever `.gguf` model you personally choose to run.

## Quick start

No prior Rust/Node/AI experience assumed — follow these in order.

**Prerequisites (only needed to *build* the app, not to run it afterward):**
- Windows 10 or 11.
- [Rust](https://rustup.rs/) — run the installer, accept the defaults. Check it worked by opening a new terminal and running `rustc --version`; it should print a version number, not "command not found."
- [Node.js](https://nodejs.org/) (LTS version) — same idea: `node --version` should print a number afterward.
- That's it. WebView2 (the native window renderer) is already built into Windows 10/11, so nothing extra is needed to *run* the built app — only to build it.

**Step 1 — Check how much RAM your machine has.** Right-click the taskbar → Task Manager → Performance tab → Memory. Note the total (e.g. "8.0 GB"). You'll need this in Step 4.

**Step 2 — Clone this repo and build the app:**
```bash
git clone https://github.com/182004jaideep/offline-ai-chat.git
cd offline-ai-chat/app
npm install
npm run tauri build
```
This takes a few minutes the first time (it's compiling the Rust backend). When it finishes without errors, the built executable is at `app/src-tauri/target/release/app.exe`. If something fails here, see [Troubleshooting](#troubleshooting) before continuing.

**Step 3 — Move the built app into place.** Copy `app.exe` out of that `target/` folder up to the repo root (`offline-ai-chat/`), next to `models/` and `setup.bat`. Rename it to `OfflineAI.exe` if you'd like a friendlier name. This is what makes the final result a single self-contained folder you can carry on a USB drive.

**Step 4 — Run `setup.bat`** (double-click it, from the repo root). It fetches the llama.cpp engine binaries (MIT-licensed, from the [official releases page](https://github.com/ggml-org/llama.cpp/releases)) into an `engine/` folder, then tells you it couldn't find a model yet and points you at Step 5.

**Step 5 — Download a model and drop it into `models/`.** Go to [Choosing a model](#choosing-a-model), match the RAM number you noted in Step 1 to a row in the table, follow that model's Hugging Face link, and download the `Q4_K_M` `.gguf` file (not the `.safetensors` or other formats). Put it directly inside the `models/` folder — not a subfolder.

**Step 6 — Run the app.** Double-click `OfflineAI.exe`. A window opens; wait a few seconds for "Engine ready" in the top-right status. From a terminal instead, you can pass `--list-models` to see what it found, or `--model <name>` to pick a specific one.

**Step 7 — Pick a privacy mode** in the window that opens (Normal or Disappearing — see [Privacy modes](#privacy-modes)), then start chatting. Closing the window stops everything cleanly — no background process is left running.

## Choosing a model

Any [GGUF](https://huggingface.co/docs/hub/gguf)-format model works. With no `--model` flag, the app loads the first `.gguf` file it finds in `models/`. Pick the row matching your *total* system RAM (not "free" RAM) from Step 1 of [Quick start](#quick-start):

| Total system RAM | Model | License | Approx. size (Q4_K_M) |
|---|---|---|:---:|
| **4 GB** (old/budget laptops) | [Qwen2.5-0.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF) | Apache-2.0 | ~0.5 GB |
| **8 GB** | [Qwen2.5-Coder-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF) | Apache-2.0 | ~1 GB |
| **16 GB**, balanced / coding | [Qwen2.5-Coder-7B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF) | Apache-2.0 | ~4.5 GB |
| **16 GB**, general-purpose chat | [Meta-Llama-3.1-8B-Instruct-GGUF](https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF) | Llama 3.1 Community License† | ~4.9 GB |

† See [Licensing & who can use this](#licensing--who-can-use-this) — free to use, but not plain MIT/Apache; has a usage condition above 700M MAU.

Pick the `Q4_K_M` quantization specifically unless you know you want something else — it's the standard balance of quality vs. size/speed for CPU-only inference. As a rule of thumb, budget the model's file size plus roughly 20–30% for the KV cache and runtime overhead, on top of whatever Windows and anything else running already needs; that's why an "8 GB" machine should run a ~1 GB model, not one close to 8 GB itself. The app warns (but doesn't block) if your selected model(s) need more RAM than is currently available.

## Limitations

This project makes local, private AI possible on ordinary hardware — but it's a genuinely different trade-off from a cloud assistant, not a drop-in replacement. Specifically:

- **Noticeably less capable than frontier cloud models** (GPT/Claude/Gemini-class) on complex reasoning, long multi-step tasks, and up-to-date knowledge — these are much smaller, quantized, CPU-run models.
- **Slower than cloud inference.** Expect single-digit-to-low-tens of tokens/second on a typical CPU, not the near-instant streaming of a data-center GPU.
- **No internet access, by design** — the model only knows what's baked into its training data (generally a year or more stale) and can't browse, search, or fetch live information.
- **No tool use or code execution** — in Coding mode especially, the model can write code but cannot run it, access your filesystem, or verify its own output; always treat suggestions as unverified until you test them yourself.
- **Smaller practical context window** on CPU — technically supported context sizes (4K–16K+ tokens) get slow well before their stated maximum on modest hardware.
- **Higher hallucination risk** than larger models, especially the smallest (0.5B–1.5B) tier — good for quick/simple tasks, not for anything where a confidently wrong answer is costly.
- **Single-user, single-machine** — there's no multi-user concurrency, auth, or scaling story here; it's a personal tool, not a hosted service.

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
Any reasonably modern CPU, and as little as 4 GB of RAM for the smallest model — see the full [Choosing a model](#choosing-a-model) table for what fits your machine. A GPU isn't required — this ships with a CPU-only engine by default, though llama.cpp also publishes CUDA/Vulkan builds if you want to accelerate it manually.

**Does it work on macOS or Linux?**
The app is built on Tauri, which is cross-platform in principle, but this repo currently only builds and is tested for Windows (the bundled engine binaries, `setup.bat`, and the drive-loss watchdog are all Windows-specific right now). Cross-platform support is on the [Roadmap](#roadmap).

## Roadmap

- [ ] Packaged, one-command release build (today you build from source with Rust + Node)
- [ ] macOS / Linux build targets
- [ ] `.pptx` and other document formats for offline search
- [ ] GPU-accelerated engine option (CUDA/Vulkan builds are already published upstream by llama.cpp)

## Contributing

Contributions are welcome — this is an early-stage project with real gaps to fill. A few places that would help most right now:

- **Cross-platform builds** (macOS/Linux) — the single highest-value contribution; see [Roadmap](#roadmap).
- **More document formats** for offline search (`.pptx` and others — `.txt`/`.md`/`.pdf`/`.docx`/`.xlsx` are already supported).
- **A packaged release build** — a one-command installer/bundle instead of building from source.
- **Bug reports** — if the build, `setup.bat`, or the app itself fails on your machine, open an issue with your Windows version and the exact error text.

Full ground rules (what must never change, like "nothing lands on the host system"), the dev setup (`npm run tauri dev`), and how to test a real portable build are all in **[CONTRIBUTING.md](CONTRIBUTING.md)** — read that before opening a PR. No CLA, no account, no gatekeeping beyond normal code review.

## Credits

- [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT License) — the inference engine this project wraps. All model execution, quantization, and the OpenAI-compatible API are its work, not this repo's.
- [Tauri](https://tauri.app/) (MIT/Apache-2.0) — the native app shell (Rust backend + OS webview).
- Model weights are the property of their respective creators (Qwen, Meta, etc.) under their own licenses — check each model card before use.

## License

MIT — see [LICENSE](LICENSE).
