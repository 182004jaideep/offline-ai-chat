# Offline AI Chat

**Run a private, offline AI chat assistant on Windows — a local ChatGPT alternative that works with no internet connection, no account, and no data ever leaving your machine.**

Double-click one file and you get a local LLM running in your browser, powered by [llama.cpp](https://github.com/ggml-org/llama.cpp), with a lightweight custom chat UI served straight from a local file. No sign-up, no API key, no subscription, no telemetry — everything runs on your own hardware.

![platform](https://img.shields.io/badge/platform-Windows-blue) ![license](https://img.shields.io/badge/license-MIT-green) ![status](https://img.shields.io/badge/status-early--stage-orange)

### Contents

[Why offline AI](#why) · [Offline AI Chat vs. cloud AI](#offline-ai-chat-vs-cloud-ai) · [Quick start](#quick-start) · [Choosing a model](#choosing-a-model) · [How it works](#how-it-works) · [Troubleshooting](#troubleshooting) · [FAQ](#faq) · [Roadmap](#roadmap)

## Why

Most "AI chat" tools ship your prompts to someone else's server. This one doesn't:

- **100% offline** — once the model is downloaded, no internet connection is required to chat.
- **Zero-trace by design** — runs Edge in `--inprivate` mode with a throwaway profile directory that is deleted on exit.
- **Auto-tuned to your hardware** — probes CPU core count and RAM at launch and picks sensible context size / thread count automatically.
- **No install** — everything lives in one folder; nothing is written to the registry or `%AppData%`.
- **OpenAI-compatible local API** — `llama-server` exposes a `/v1/chat/completions` endpoint on `127.0.0.1`, so you can point other local tools at it too.

## Offline AI Chat vs. cloud AI

| | Offline AI Chat | Typical cloud AI chat |
|---|---|---|
| Works without internet | ✅ Yes, after the one-time model download | ❌ No |
| Your prompts leave your device | ❌ Never | ✅ Sent to a remote server |
| Account / sign-up required | ❌ No | ✅ Usually |
| Ongoing subscription cost | ❌ Free, runs on your own hardware | 💲 Often paid |
| Data retained after you close it | ❌ Nothing — browser profile is shredded on exit | ⚠️ Depends on the provider's policy |
| Works in flight / no-wifi / secure environments | ✅ Yes | ❌ No |

## Quick start

**Prerequisites:** Windows 10/11, Microsoft Edge installed, ~6 GB of free disk space (more depending on the model you pick).

1. **Clone or download this repo.**
   ```bash
   git clone https://github.com/182004jaideep/offline-ai-chat.git
   cd offline-ai-chat
   ```
2. **Run `setup.bat`.** It downloads the llama.cpp inference engine for Windows (MIT-licensed, fetched straight from the [official releases page](https://github.com/ggml-org/llama.cpp/releases)) and tells you exactly which model to grab next.
3. **Download a model** (one-time) and drop the `.gguf` file into the `models/` folder. `setup.bat` prints direct links and picks for you based on RAM — or see [Choosing a model](#choosing-a-model) below.
4. **Double-click `Run-Office-AI.bat`.** It detects your hardware, starts the local engine, waits for it to be ready, then opens the chat UI in a private Edge window.
5. **Close the Edge window when you're done.** The script kills the engine and shreds the temporary browser profile automatically.

## Choosing a model

Any [GGUF](https://huggingface.co/docs/hub/gguf)-format model works — `Run-Office-AI.bat` automatically picks the largest `.gguf` file in `models/`. Some starting points:

| Use case            | Model                                                                                             | Approx. size | RAM needed |
|----------------------|-----------------------------------------------------------------------------------------------------|:------------:|:----------:|
| Low-end machines     | [Qwen2.5-Coder-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF)    | ~1 GB        | 8 GB       |
| Balanced / coding     | [Qwen2.5-Coder-7B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF)        | ~4.5 GB (Q4_K_M) | 16 GB |
| General-purpose chat | [Meta-Llama-3.1-8B-Instruct-GGUF](https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF) | ~4.9 GB (Q4_K_M) | 16 GB |

Pick a `Q4_K_M` quantization unless you know you want something else — it's the standard quality/size tradeoff.

## How it works

```
Run-Office-AI.bat
   │
   ├─ 1. Probes CPU cores + RAM via PowerShell (Win32_Processor / Win32_OperatingSystem)
   ├─ 2. Picks the largest .gguf in models/, derives context size + thread count
   ├─ 3. Launches llama-server.exe on 127.0.0.1:8585 (OpenAI-compatible API)
   ├─ 4. Waits for /health to report "ok"
   ├─ 5. Opens Edge in --inprivate mode, pointed at index.html, with a throwaway
   │      --user-data-dir under temp/
   ├─ 6. index.html talks to the local API (fetch + streaming SSE) — nothing
   │      ever leaves 127.0.0.1
   └─ 7. On window close: kills llama-server.exe, deletes temp/
```

## Project structure

```
offline-ai-chat/
├── Run-Office-AI.bat   # Main launcher: hardware probe → engine → browser UI
├── setup.bat           # One-time helper: downloads the llama.cpp engine
├── index.html           # Chat UI (vanilla HTML/CSS/JS, no build step)
├── models/              # Put your .gguf model file(s) here (gitignored)
├── llama.cpp.txt        # License text for the bundled llama.cpp binaries
├── LICENSE
└── README.md
```

`llama-server.exe` and its DLLs, plus every `.gguf` model, are intentionally **not** committed to this repo — GitHub blocks files over 100 MB, and vendoring binaries would make every clone multiple gigabytes. `setup.bat` fetches the engine for you; the model download is manual because the "right" model depends on your hardware.

## Troubleshooting

- **"This file is corrupted" / weird characters when opening `index.html`** — this is almost always filesystem corruption on the drive the project lives on, not a bug in the script. This is especially common if the project sits on a cheap/flaky USB flash drive formatted exFAT (no journaling, so unsafe removal or power loss corrupts files). Run `chkdsk <drive>: /F` and, if it recurs, copy the project to a local SSD instead.
- **"No .gguf model found"** — make sure the model file is directly inside `models/`, not a subfolder, and that it has a `.gguf` extension.
- **Port 8585 already in use** — the script automatically kills any stray `llama-server.exe` holding that port before launching; if it still fails, check what else is bound to `127.0.0.1:8585`.
- **Edge isn't installed** — this currently requires Microsoft Edge specifically (launched via `msedge --app`). See [Roadmap](#roadmap) for browser-agnostic plans.

## FAQ

**Is this really free?**
Yes. The project is MIT-licensed, llama.cpp is MIT-licensed, and the models linked above are free to download. You only need hardware capable of running them — no subscription, no API key, no account.

**Does it actually work with no internet connection?**
Yes, once `setup.bat` has fetched the engine and you've downloaded a `.gguf` model. From that point on, `Run-Office-AI.bat` never makes a network call — `llama-server` and the chat UI both run entirely on `127.0.0.1`.

**Is my data really private with offline AI?**
Nothing you type is sent anywhere — there's no server to send it to. The chat UI talks only to `127.0.0.1:8585` on the same machine, and the Edge browser profile it runs in is deleted the moment you close the window.

**How is this different from ChatGPT or other cloud AI assistants?**
Cloud assistants run the model on someone else's servers and your prompts travel over the internet to get there. Offline AI Chat runs the same kind of model (an open-weight LLM via llama.cpp) locally on your CPU/RAM — slower per token than a data-center GPU, but nothing ever leaves your device, there's no account, and it keeps working with no internet at all.

**What hardware do I need to run an LLM locally?**
A reasonably modern CPU and 8–16 GB of RAM covers the models listed in [Choosing a model](#choosing-a-model). A GPU isn't required — this ships with a CPU-only engine by default, though llama.cpp also publishes CUDA/Vulkan builds if you want to accelerate it manually.

**Does it work on macOS or Linux?**
Not yet — the launcher is currently Windows-only (`.bat` + PowerShell + Edge). Cross-platform support is the top item on the [Roadmap](#roadmap) and a great first contribution; see [CONTRIBUTING.md](CONTRIBUTING.md).

**Why Microsoft Edge specifically?**
It's the only browser currently wired up for the `--inprivate` throwaway-profile launch. Browser-agnostic launching (Chrome, Firefox, your system default) is on the roadmap.

## Roadmap

This project is Windows-only today (batch script + PowerShell + `msedge`). Contributions are very welcome for:

- [ ] macOS / Linux launcher (shell script equivalent, using `/proc` or `sysctl` for hardware probing)
- [ ] Browser-agnostic launch (fall back to Chrome or default browser if Edge isn't present)
- [ ] GPU-accelerated engine option (CUDA/Vulkan builds are already published upstream by llama.cpp)
- [ ] Packaged releases (zip bundle with engine pre-included, via GitHub Releases)
- [ ] Model manager UI (browse/download/switch models from within the chat UI)

See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## Credits

- [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT License) — the inference engine this project wraps. All model execution, quantization, and the OpenAI-compatible API are its work, not this repo's.
- Model weights are the property of their respective creators (Qwen, Meta, etc.) under their own licenses — check each model card before use.

## License

MIT — see [LICENSE](LICENSE). This project is a thin, user-friendly wrapper around llama.cpp; use it, fork it, ship it.
