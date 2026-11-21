mod db;
mod ephemeral;
mod models;
mod watchdog;

use std::collections::VecDeque;
use std::env;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
use ephemeral::EphemeralStore;
use models::{format_size, resolve_requested, scan_models, ModelInfo};
use rusqlite::Connection;
use tauri::{Emitter, Manager};
use watchdog::DriveLost;

/// How many recent lines of an engine's combined stdout/stderr to keep
/// around, so a crash can be explained (in a warning, or in the Models
/// tab) instead of just silently going quiet.
const LOG_RING_SIZE: usize = 200;
type LogBuffer = Arc<Mutex<VecDeque<String>>>;

const ENGINE_HOST: &str = "127.0.0.1";
const BASE_ENGINE_PORT: u16 = 8585;
/// Fraction of available RAM the selected models are allowed to occupy
/// before we warn the user. Not a hard cap -- on an offline box with no
/// cloud fallback, the user should be able to override, just never
/// silently.
const RAM_WARN_THRESHOLD: f64 = 0.9;

#[derive(Parser, Debug)]
#[command(
    name = "Offline AI",
    about = "Portable, offline-only personal AI assistant",
    long_about = "Runs entirely from this drive. Model selection, document ingestion, and search all happen here, at launch, from the terminal -- nothing is fetched from the internet."
)]
struct Cli {
    /// Load a specific model by name (its file name without the .gguf
    /// extension). Repeat to load several models at once, each on its own
    /// local port. If omitted, the first model found in models/ is used.
    #[arg(long = "model")]
    model: Vec<String>,

    /// List the .gguf models found in models/ and exit without opening the
    /// window -- for checking what's on the drive from the terminal.
    #[arg(long = "list-models")]
    list_models: bool,

    /// Ingest a document (.txt/.md) into the offline search index and
    /// exit. Repeat to ingest several files in one run.
    #[arg(long = "ingest")]
    ingest: Vec<String>,

    /// List ingested documents and exit.
    #[arg(long = "list-documents")]
    list_documents: bool,

    /// Run an offline keyword search against ingested documents and exit.
    #[arg(long = "search")]
    search: Option<String>,

    /// Override the context size (in tokens) passed to llama-server for
    /// every loaded model. If omitted, a value is chosen from this
    /// machine's total RAM.
    #[arg(long = "ctx")]
    ctx: Option<u32>,
}

struct LoadedModel {
    name: String,
    port: u16,
    child: Child,
    log: LogBuffer,
    /// The context size this specific instance was actually spawned with
    /// -- tracked per-instance (not just a global default) since Feature 3
    /// lets a live `load_model` call ask for a different context than the
    /// one picked at launch.
    ctx: u32,
}

/// Reads lines from a child process stream into a shared ring buffer.
/// Runs on its own thread for the lifetime of the pipe; exits cleanly when
/// the child closes the stream (on exit or crash).
fn spawn_log_drain(stream: impl Read + Send + 'static, log: LogBuffer) {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Ok(mut buf) = log.lock() {
                buf.push_back(line);
                while buf.len() > LOG_RING_SIZE {
                    buf.pop_front();
                }
            }
        }
    });
}

fn log_tail(log: &LogBuffer, n: usize) -> String {
    log.lock()
        .map(|buf| {
            let start = buf.len().saturating_sub(n);
            buf.iter().skip(start).cloned().collect::<Vec<_>>().join(" | ")
        })
        .unwrap_or_default()
}

/// Removes any engine whose process has exited, logging why. Called before
/// every command that reads "what's loaded" (so stale state never leaks
/// out) and from a background watcher thread (so a crash is reported
/// proactively, not only the next time the UI happens to ask).
const MAX_AUTO_RESTARTS: usize = 3;
const AUTO_RESTART_WINDOW_MS: i64 = 5 * 60 * 1000;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Decides whether a just-died model is still allowed an automatic
/// restart attempt, and records this attempt if so. A hard cap (3 per
/// 5-minute window per model) is the entire safety mechanism against a
/// crash-loop: once exceeded, this always returns false and the model
/// falls back to the normal Failed+Retry state instead of looping.
fn note_restart_attempt(state: &EngineState, name: &str) -> bool {
    let Ok(mut attempts) = state.restart_attempts.lock() else {
        return false;
    };
    let now = now_millis();
    let history = attempts.entry(name.to_string()).or_default();
    history.retain(|&t| now - t < AUTO_RESTART_WINDOW_MS);
    if history.len() >= MAX_AUTO_RESTARTS {
        return false;
    }
    history.push(now);
    true
}

fn reap_dead_engines(state: &EngineState, app_handle: Option<&tauri::AppHandle>) {
    let mut loaded = match state.loaded.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let mut i = 0;
    while i < loaded.len() {
        match loaded[i].child.try_wait() {
            Ok(Some(status)) => {
                let dead = loaded.remove(i);
                let tail = log_tail(&dead.log, 5);
                let exit_code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                // Try one automatic respawn (same model, same context)
                // before falling back to the Failed state -- capped so a
                // model that's persistently broken (bad file, genuinely
                // not enough RAM) doesn't loop forever.
                if note_restart_attempt(state, &dead.name) {
                    let root = drive_root();
                    let registry = scan_models(&root);
                    let server_exe = engine_exe_path(&root);
                    if let Some(model) = registry.iter().find(|m| m.name == dead.name) {
                        let port = next_free_port(ENGINE_HOST, dead.port);
                        let (_, threads) = compute_hw_params();
                        match spawn_engine_process(&server_exe, &root, model, port, dead.ctx, threads) {
                            Ok((child, log)) => {
                                let msg = format!(
                                    "Model '{}' crashed (exit code {}) and was automatically restarted.",
                                    dead.name, exit_code
                                );
                                eprintln!("[offline-ai] {}", msg);
                                if let Ok(mut warnings) = state.warnings.lock() {
                                    warnings.push(msg.clone());
                                }
                                loaded.insert(
                                    i,
                                    LoadedModel {
                                        name: dead.name.clone(),
                                        port,
                                        child,
                                        log,
                                        ctx: dead.ctx,
                                    },
                                );
                                if let Some(app) = app_handle {
                                    let _ = app.emit(
                                        "engine-auto-restarted",
                                        serde_json::json!({ "name": dead.name, "port": port, "message": msg }),
                                    );
                                }
                                i += 1;
                                continue;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[offline-ai] auto-restart of '{}' failed to spawn: {}",
                                    dead.name, e
                                );
                                // fall through to the normal Failed path below
                            }
                        }
                    }
                }

                let msg = format!(
                    "Model '{}' stopped unexpectedly (exit code {}). {}",
                    dead.name,
                    exit_code,
                    if tail.is_empty() {
                        "(no output captured)".to_string()
                    } else {
                        format!("Last output: {}", tail)
                    }
                );
                eprintln!("[offline-ai] warning: {}", msg);
                if let Ok(mut warnings) = state.warnings.lock() {
                    warnings.push(msg.clone());
                }
                if let Ok(mut errors) = state.last_errors.lock() {
                    errors.insert(dead.name.clone(), msg.clone());
                }
                if let Some(app) = app_handle {
                    let _ = app.emit(
                        "engine-exited",
                        serde_json::json!({ "name": dead.name, "message": msg }),
                    );
                }
            }
            Ok(None) => i += 1,
            Err(_) => i += 1, // can't determine -- leave it, don't false-alarm
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct ActiveModel {
    name: String,
    port: u16,
    ctx: u32,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelRegistryEntry {
    name: String,
    size_bytes: u64,
    loaded: bool,
    port: Option<u16>,
    last_error: Option<String>,
}

struct EngineState {
    loaded: Mutex<Vec<LoadedModel>>,
    warnings: Mutex<Vec<String>>,
    /// Why a model's engine most recently died, keyed by model name.
    /// Survives `reap_dead_engines` removing the dead entry from `loaded`,
    /// so the Models tab can show a real Failed state (with the reason)
    /// instead of the row just quietly reverting to "not loaded" as if
    /// nothing happened. Cleared the moment a reload is attempted.
    last_errors: Mutex<std::collections::HashMap<String, String>>,
    /// Recent auto-restart timestamps (millis) per model name -- the
    /// entire crash-loop safety mechanism: see `note_restart_attempt`.
    restart_attempts: Mutex<std::collections::HashMap<String, Vec<i64>>>,
}

struct DbState(Mutex<Connection>);

fn db_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Checked at the top of every database-touching command. Once the drive
/// is gone, failing fast with one clear message beats letting SQLite throw
/// a confusing I/O error from a half-vanished file handle.
fn check_drive(drive: &tauri::State<DriveLost>) -> Result<(), String> {
    if drive.is_lost() {
        Err("The drive this app runs from is no longer connected. Nothing can be read or saved until it's reconnected and the app is restarted.".to_string())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn get_active_models(state: tauri::State<EngineState>) -> Vec<ActiveModel> {
    reap_dead_engines(&state, None);
    state
        .loaded
        .lock()
        .map(|guard| {
            guard
                .iter()
                .map(|m| ActiveModel {
                    name: m.name.clone(),
                    port: m.port,
                    ctx: m.ctx,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
fn get_warnings(state: tauri::State<EngineState>) -> Vec<String> {
    state.warnings.lock().map(|g| g.clone()).unwrap_or_default()
}

/// The full picture for the "Models" sidebar tab: every .gguf found on the
/// drive, cross-referenced with which ones currently have a running
/// engine. This is what makes model selection something you do from the
/// UI rather than only via `--model` at launch.
#[tauri::command]
fn list_model_registry(state: tauri::State<EngineState>) -> Vec<ModelRegistryEntry> {
    reap_dead_engines(&state, None);
    let root = drive_root();
    let registry = scan_models(&root);
    let loaded: Vec<(String, u16)> = state
        .loaded
        .lock()
        .map(|guard| guard.iter().map(|m| (m.name.clone(), m.port)).collect())
        .unwrap_or_default();
    let errors = state.last_errors.lock().map(|g| g.clone()).unwrap_or_default();

    registry
        .into_iter()
        .map(|m| {
            let active = loaded.iter().find(|(name, _)| *name == m.name);
            ModelRegistryEntry {
                last_error: if active.is_some() { None } else { errors.get(&m.name).cloned() },
                name: m.name,
                size_bytes: m.size_bytes,
                loaded: active.is_some(),
                port: active.map(|(_, port)| *port),
            }
        })
        .collect()
}

#[tauri::command]
fn load_model(
    name: String,
    ctx_override: Option<u32>,
    state: tauri::State<EngineState>,
    drive: tauri::State<DriveLost>,
) -> Result<ActiveModel, String> {
    check_drive(&drive)?;
    reap_dead_engines(&state, None);
    let root = drive_root();
    let registry = scan_models(&root);
    let model = registry
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("unknown model: {}", name))?;

    let server_exe = engine_exe_path(&root);
    if !server_exe.exists() {
        return Err(format!("llama-server.exe not found at {:?}", server_exe));
    }

    let mut loaded = state.loaded.lock().map_err(db_err)?;
    if loaded.iter().any(|m| m.name == name) {
        return Err(format!("'{}' is already loaded", name));
    }

    // Retrying clears the old Failed state immediately -- if it fails
    // again, reap_dead_engines will record the new reason.
    if let Ok(mut errors) = state.last_errors.lock() {
        errors.remove(&name);
    }

    let (auto_ctx, threads) = compute_hw_params();
    let ctx = ctx_override.unwrap_or(auto_ctx);

    // RAM check against everything that would be loaded at once, including
    // this one -- a warning, not a block, same policy as at launch. Each
    // already-loaded instance contributes its own weights *and* its own
    // KV-cache overhead (it may have been loaded with a different context
    // size than this new request).
    let already_loaded_size: u64 = loaded
        .iter()
        .map(|lm| {
            let weights = registry
                .iter()
                .find(|r| r.name == lm.name)
                .map(|r| r.size_bytes)
                .unwrap_or(0);
            weights + estimate_kv_cache_overhead(lm.ctx)
        })
        .sum();
    {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        let available = sys.available_memory();
        let required = already_loaded_size + model.size_bytes + estimate_kv_cache_overhead(ctx);
        if available > 0 && required as f64 > available as f64 * RAM_WARN_THRESHOLD {
            if let Ok(mut warnings) = state.warnings.lock() {
                warnings.push(format!(
                    "Loading '{}' (context {}) brings total estimated memory to ~{}, but only ~{} of RAM is available -- the engine may swap heavily or fail.",
                    name,
                    ctx,
                    format_size(required),
                    format_size(available)
                ));
            }
        }
    }

    let start_port = loaded.iter().map(|m| m.port + 1).max().unwrap_or(BASE_ENGINE_PORT);
    let port = next_free_port(ENGINE_HOST, start_port);

    let (child, log) = spawn_engine_process(&server_exe, &root, model, port, ctx, threads)
        .map_err(|e| format!("failed to launch '{}': {}", name, e))?;

    eprintln!(
        "[offline-ai] loaded '{}' on port {} (from UI, ctx={}, threads={})",
        name, port, ctx, threads
    );
    loaded.push(LoadedModel {
        name: name.clone(),
        port,
        child,
        log,
        ctx,
    });

    Ok(ActiveModel { name, port, ctx })
}

#[tauri::command]
fn unload_model(name: String, state: tauri::State<EngineState>) -> Result<(), String> {
    let mut loaded = state.loaded.lock().map_err(db_err)?;
    let pos = loaded
        .iter()
        .position(|m| m.name == name)
        .ok_or_else(|| format!("'{}' is not loaded", name))?;
    let mut model = loaded.remove(pos);
    let _ = model.child.kill();
    eprintln!("[offline-ai] unloaded '{}'", name);
    Ok(())
}

#[tauri::command]
fn create_session(
    title: String,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<i64, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::create_session(&conn, &title).map_err(db_err)
}

#[tauri::command]
fn list_sessions(
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<db::SessionRow>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::list_sessions(&conn).map_err(db_err)
}

#[tauri::command]
fn save_message(
    session_id: i64,
    role: String,
    content: String,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<(), String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::save_message(&conn, session_id, &role, &content).map_err(db_err)
}

#[tauri::command]
fn get_messages(
    session_id: i64,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<db::MessageRow>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::get_messages(&conn, session_id).map_err(db_err)
}

#[tauri::command]
fn delete_session(
    session_id: i64,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<(), String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::delete_session(&conn, session_id).map_err(db_err)
}

#[tauri::command]
fn search_documents(
    query: String,
    limit: i64,
    document_ids: Option<Vec<i64>>,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<db::SearchResult>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::search_documents(&conn, &query, limit, document_ids.as_deref()).map_err(db_err)
}

#[tauri::command]
fn get_session_documents(
    session_id: i64,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<i64>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::get_session_document_ids(&conn, session_id)
}

#[tauri::command]
fn list_documents(
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<db::DocumentRow>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::list_documents(&conn).map_err(db_err)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IngestResult {
    message: String,
    document_id: i64,
}

#[tauri::command]
fn pick_and_ingest_document(
    session_id: Option<i64>,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<IngestResult, String> {
    check_drive(&drive)?;
    let path = rfd::FileDialog::new()
        .add_filter(
            "Documents & code",
            &[
                "txt", "md", "markdown", "pdf", "docx", "xlsx", "xls", "xlsm", "ods", "py", "js",
                "jsx", "ts", "tsx", "rs", "go", "java", "c", "h", "cpp", "hpp", "cs", "rb", "php",
                "swift", "kt", "lua", "r", "vue", "svelte", "json", "yaml", "yml", "toml", "ini",
                "cfg", "conf", "sh", "bash", "sql", "css", "scss", "html", "xml",
            ],
        )
        .set_title("Add a document to the offline search index")
        .pick_file();
    let Some(path) = path else {
        return Err("No file selected".to_string());
    };
    let conn = db.0.lock().map_err(db_err)?;
    let (document_id, count) = db::ingest_document(&conn, &path)?;
    if let Some(sid) = session_id {
        db::link_document_to_session(&conn, sid, document_id)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    Ok(IngestResult {
        message: format!("{} ({} chunk(s) indexed)", name, count),
        document_id,
    })
}

/// Writes a code block's content to a location the user explicitly picks
/// via a native save dialog. Deliberately NOT gated by `check_drive` or
/// any privacy mode: this is a one-off, user-initiated export *out* of
/// the app (e.g. "save this generated function into my project"), not
/// part of the drive-only chat/document data path -- don't mistake it for
/// a privacy regression if you're reading this later.
#[tauri::command]
fn save_text_to_file(content: String, suggested_name: Option<String>) -> Result<String, String> {
    let mut dialog = rfd::FileDialog::new().set_title("Save code to file");
    if let Some(name) = suggested_name {
        dialog = dialog.set_file_name(&name);
    }
    let path = dialog.save_file().ok_or_else(|| "No location chosen".to_string())?;
    std::fs::write(&path, content).map_err(|e| format!("could not write file: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn delete_document(
    document_id: i64,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<(), String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::delete_document(&conn, document_id).map_err(db_err)
}

#[tauri::command]
fn add_note(
    title: String,
    body: String,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<i64, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::add_note(&conn, &title, &body).map_err(db_err)
}

#[tauri::command]
fn list_notes(
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<Vec<db::NoteRow>, String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::list_notes(&conn).map_err(db_err)
}

#[tauri::command]
fn delete_note(
    note_id: i64,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<(), String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::delete_note(&conn, note_id).map_err(db_err)
}

#[tauri::command]
fn update_note(
    note_id: i64,
    title: String,
    body: String,
    db: tauri::State<DbState>,
    drive: tauri::State<DriveLost>,
) -> Result<(), String> {
    check_drive(&drive)?;
    let conn = db.0.lock().map_err(db_err)?;
    db::update_note(&conn, note_id, &title, &body).map_err(db_err)
}

// --- disappearing mode: in-memory only, never touches the database -----

#[tauri::command]
fn start_disappearing_session(ephemeral: tauri::State<EphemeralStore>) -> String {
    ephemeral.start_session()
}

#[tauri::command]
fn disappearing_save_message(
    token: String,
    role: String,
    content: String,
    ephemeral: tauri::State<EphemeralStore>,
) -> Result<(), String> {
    ephemeral.save_message(&token, &role, &content)
}

#[tauri::command]
fn end_disappearing_session(token: String, ephemeral: tauri::State<EphemeralStore>) {
    ephemeral.end_session(&token);
}

/// Directory the running executable lives in. In a portable layout this is
/// the pendrive root (or wherever the app was copied), and every path the
/// app touches is resolved relative to it -- never %APPDATA%/%LOCALAPPDATA%/registry.
fn drive_root() -> PathBuf {
    let exe = env::current_exe().expect("failed to resolve current executable path");
    exe.parent()
        .expect("executable has no parent directory")
        .to_path_buf()
}

/// Path to the bundled llama-server.exe, kept in its own `engine/` folder
/// alongside its DLLs rather than loose at the drive root. Windows resolves
/// a DLL search in the *loading exe's* own directory, so moving
/// llama-server.exe and its DLLs together into one subfolder doesn't break
/// anything -- only `OfflineAI.exe` itself stays at the root, since that's
/// what a user double-clicks from the drive.
fn engine_exe_path(root: &Path) -> PathBuf {
    root.join("engine").join("llama-server.exe")
}

fn is_port_open(host: &str, port: u16) -> bool {
    TcpStream::connect((host, port)).is_ok()
}

fn next_free_port(host: &str, start: u16) -> u16 {
    let mut port = start;
    while is_port_open(host, port) {
        port += 1;
    }
    port
}

/// Picks a context size and thread count from this machine's hardware,
/// the way the old hardware-probe launcher did before the Tauri rewrite
/// dropped it. Without an explicit context size, llama-server falls back
/// to whatever the model file itself declares as default (often 32k+),
/// which can be far more KV-cache memory than a modest machine has to
/// spare -- this keeps that bounded.
fn compute_hw_params() -> (u32, usize) {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();

    let cores = sys.physical_core_count().unwrap_or_else(|| sys.cpus().len().max(1));
    let threads = cores.clamp(2, 8);

    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let ctx = if total_ram_gb < 12.0 { 4096 } else { 8192 };

    (ctx, threads)
}

/// A deliberately rough estimate of KV-cache memory for a given context
/// size -- NOT derived from the model's actual architecture (that would
/// need parsing GGUF metadata for layer count / embedding dimension,
/// real scope creep for what's meant to be "a reasonable estimate").
/// Before this, the RAM warning counted only model *file size*, so asking
/// for a bigger context (more code/conversation fitting in one prompt)
/// silently made the warning less accurate exactly when context size
/// mattered most. This errs generously high on purpose: it's a warning,
/// never a hard block, so over-estimating is the safe direction.
fn estimate_kv_cache_overhead(ctx: u32) -> u64 {
    const BYTES_PER_1024_CTX: u64 = 150 * 1024 * 1024; // ~150MB / 1024 tokens, rough
    (ctx as u64).div_ceil(1024) * BYTES_PER_1024_CTX
}

/// Spawns `llama-server.exe` for one model with its stdout/stderr piped
/// into a shared ring buffer (so a crash can be explained, not just
/// silently go quiet -- see `reap_dead_engines`), and with hardware-sized
/// `-c`/`-t`/`-tb` flags rather than relying on llama-server's own
/// (often much larger) defaults. Flags are verified against this
/// project's actual bundled `llama-server.exe --help` output, not carried
/// over from an older build -- that build's CLI surface has drifted
/// (`-fa` now takes a value, `--no-mmap` no longer exists).
fn spawn_engine_process(
    server_exe: &Path,
    root: &Path,
    model: &ModelInfo,
    port: u16,
    ctx: u32,
    threads: usize,
) -> std::io::Result<(Child, LogBuffer)> {
    let mut child = Command::new(server_exe)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args([
            "-m",
            model.path.to_string_lossy().as_ref(),
            "--host",
            ENGINE_HOST,
            "--port",
            &port.to_string(),
            "-c",
            &ctx.to_string(),
            "-t",
            &threads.to_string(),
            "-tb",
            &threads.to_string(),
        ])
        .spawn()?;

    let log: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stdout) = child.stdout.take() {
        spawn_log_drain(stdout, log.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_drain(stderr, log.clone());
    }

    Ok((child, log))
}

#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // Best-effort: lets the terminal-only flags print into the caller's
    // terminal even in a release (GUI-subsystem) build. If there is no
    // parent console (double-clicked from Explorer), this just fails
    // silently and the app continues as a normal GUI launch.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// Picks a default model when the user didn't pass `--model`: the largest
/// one that comfortably fits in currently available RAM, or the smallest
/// overall if none do. This matters for a true "double-click and it just
/// works" launch -- picking alphabetically-first regardless of size could
/// pick a model too big for the machine while a smaller one that would
/// have run fine sits right next to it.
fn pick_default_model(registry: &[ModelInfo], ctx: u32) -> Vec<&ModelInfo> {
    if registry.is_empty() {
        return Vec::new();
    }

    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let available = sys.available_memory();
    if available == 0 {
        // Couldn't read memory info -- fall back to simple first-found.
        return registry.iter().take(1).collect();
    }

    let overhead = estimate_kv_cache_overhead(ctx);
    let budget = ((available as f64 * RAM_WARN_THRESHOLD) as u64).saturating_sub(overhead);
    let mut by_size: Vec<&ModelInfo> = registry.iter().collect();
    by_size.sort_by_key(|m| m.size_bytes);

    let best_fit = by_size.iter().rev().find(|m| m.size_bytes <= budget);
    match best_fit {
        Some(m) => vec![*m],
        None => vec![by_size[0]], // nothing fits comfortably -- try the smallest anyway
    }
}

fn check_ram(selected: &[&ModelInfo], ctx: u32) -> Option<String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let available = sys.available_memory();
    if available == 0 {
        return None; // couldn't read memory info -- don't block on it
    }
    let overhead = estimate_kv_cache_overhead(ctx) * selected.len() as u64;
    let required: u64 = selected.iter().map(|m| m.size_bytes).sum::<u64>() + overhead;
    if required as f64 > available as f64 * RAM_WARN_THRESHOLD {
        Some(format!(
            "Selected model(s) at context {} need ~{} but only ~{} of RAM is available -- the engine may swap heavily or fail to load.",
            ctx,
            format_size(required),
            format_size(available)
        ))
    } else {
        None
    }
}

/// Spawns one llama-server process per selected model, each on its own
/// port. A failure loading one model is reported as a warning and does not
/// prevent the others from loading.
fn spawn_models(
    root: &Path,
    selected: &[&ModelInfo],
    ctx_override: Option<u32>,
) -> (Vec<LoadedModel>, Vec<String>) {
    let server_exe = engine_exe_path(root);
    let mut loaded = Vec::new();
    let mut warnings = Vec::new();

    if !server_exe.exists() {
        warnings.push(format!(
            "llama-server.exe not found at {:?} -- backend cannot start",
            server_exe
        ));
        return (loaded, warnings);
    }

    let (auto_ctx, threads) = compute_hw_params();
    let ctx = ctx_override.unwrap_or(auto_ctx);

    let mut port = BASE_ENGINE_PORT;
    for model in selected {
        port = next_free_port(ENGINE_HOST, port);
        let spawned = spawn_engine_process(&server_exe, root, model, port, ctx, threads);

        match spawned {
            Ok((child, log)) => {
                eprintln!(
                    "[offline-ai] loaded '{}' on port {} (ctx={}, threads={})",
                    model.name, port, ctx, threads
                );
                loaded.push(LoadedModel {
                    name: model.name.clone(),
                    port,
                    child,
                    log,
                    ctx,
                });
                port += 1;
            }
            Err(e) => {
                warnings.push(format!("failed to launch model '{}': {}", model.name, e));
            }
        }
    }

    (loaded, warnings)
}

fn kill_engine(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<EngineState>();
    if let Ok(mut guard) = state.loaded.lock() {
        for mut model in guard.drain(..) {
            let _ = model.child.kill();
            eprintln!("[offline-ai] stopped '{}'", model.name);
        }
    }
}

/// Handles the terminal-only actions (`--list-models`, `--ingest`,
/// `--list-documents`, `--search`) that print to stdout/stderr and exit
/// without ever opening the window. Returns `true` if one of them ran.
fn run_terminal_actions(cli: &Cli, root: &Path, registry: &[ModelInfo]) -> bool {
    if cli.list_models {
        if registry.is_empty() {
            println!("No .gguf models found in {:?}", models::models_dir(root));
        } else {
            println!("Available models in {:?}:", models::models_dir(root));
            for m in registry {
                println!("  {}  ({})", m.name, format_size(m.size_bytes));
            }
        }
        return true;
    }

    if !cli.ingest.is_empty() {
        let conn = match db::open(root) {
            Ok((c, warning)) => {
                if let Some(w) = warning {
                    eprintln!("[offline-ai] warning: {}", w);
                }
                c
            }
            Err(e) => {
                eprintln!("[offline-ai] could not open database: {}", e);
                return true;
            }
        };
        for raw_path in &cli.ingest {
            let path = PathBuf::from(raw_path);
            match db::ingest_document(&conn, &path) {
                Ok((_, count)) => println!("Ingested {:?} -- {} chunk(s) indexed", path, count),
                Err(e) => eprintln!("Failed to ingest {:?}: {}", path, e),
            }
        }
        return true;
    }

    if cli.list_documents {
        let conn = match db::open(root) {
            Ok((c, warning)) => {
                if let Some(w) = warning {
                    eprintln!("[offline-ai] warning: {}", w);
                }
                c
            }
            Err(e) => {
                eprintln!("[offline-ai] could not open database: {}", e);
                return true;
            }
        };
        match db::list_documents(&conn) {
            Ok(docs) if docs.is_empty() => println!("No documents ingested yet."),
            Ok(docs) => {
                for d in docs {
                    println!("  {} -- {} chunk(s) -- {}", d.name, d.chunk_count, d.source_path);
                }
            }
            Err(e) => eprintln!("[offline-ai] could not list documents: {}", e),
        }
        return true;
    }

    if let Some(query) = &cli.search {
        let conn = match db::open(root) {
            Ok((c, warning)) => {
                if let Some(w) = warning {
                    eprintln!("[offline-ai] warning: {}", w);
                }
                c
            }
            Err(e) => {
                eprintln!("[offline-ai] could not open database: {}", e);
                return true;
            }
        };
        match db::search_documents(&conn, query, 5, None) {
            Ok(results) if results.is_empty() => println!("No matches for {:?}", query),
            Ok(results) => {
                for r in results {
                    println!("--- {} ---\n{}\n", r.document_name, r.content);
                }
            }
            Err(e) => eprintln!("[offline-ai] search failed: {}", e),
        }
        return true;
    }

    false
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if env::args().count() > 1 {
        attach_parent_console();
    }

    let cli = Cli::parse();
    let root = drive_root();
    let registry = scan_models(&root);

    if run_terminal_actions(&cli, &root, &registry) {
        return;
    }

    let (auto_ctx, _) = compute_hw_params();
    let effective_ctx = cli.ctx.unwrap_or(auto_ctx);

    let selected: Vec<&ModelInfo> = if cli.model.is_empty() {
        let picked = pick_default_model(&registry, effective_ctx);
        if let Some(m) = picked.first() {
            eprintln!(
                "[offline-ai] no --model given -- auto-picked '{}' ({}) to fit available RAM",
                m.name,
                format_size(m.size_bytes)
            );
        }
        picked
    } else {
        match resolve_requested(&registry, &cli.model) {
            Ok(models) => models,
            Err(unknown) => {
                eprintln!(
                    "[offline-ai] unknown model name(s): {} -- run with --list-models to see what's available",
                    unknown.join(", ")
                );
                Vec::new()
            }
        }
    };

    let mut warnings = Vec::new();
    if selected.is_empty() {
        warnings.push(
            "No model selected -- place a .gguf file in models/ or pass --model <name>."
                .to_string(),
        );
    } else if let Some(w) = check_ram(&selected, effective_ctx) {
        warnings.push(w);
    }

    let (loaded, mut spawn_warnings) = if selected.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        spawn_models(&root, &selected, cli.ctx)
    };
    warnings.append(&mut spawn_warnings);

    for w in &warnings {
        eprintln!("[offline-ai] warning: {}", w);
    }

    // Keep all WebView2 state (cache, cookies, local storage) on the drive,
    // never in the host's %LOCALAPPDATA% -- this is the one place a
    // webview-based shell normally leaks data onto the host system.
    let webview_data_dir = root.join(".runtime").join("webview2");
    let _ = std::fs::create_dir_all(&webview_data_dir);
    // SAFETY: called once, synchronously, before any other thread exists
    // (Tauri's webview is created after `run()` is called below).
    unsafe {
        env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_data_dir);
    }

    let db_conn = match db::open(&root) {
        Ok((c, recovery_warning)) => {
            if let Some(w) = &recovery_warning {
                eprintln!("[offline-ai] warning: {}", w);
                warnings.push(w.clone());
            }
            c
        }
        Err(e) => {
            eprintln!("[offline-ai] could not open database: {} -- chat history and document search will not work this session", e);
            // Fall back to a private in-memory database so the app still
            // runs rather than crashing outright.
            Connection::open_in_memory().expect("in-memory sqlite must open")
        }
    };

    let engine = EngineState {
        loaded: Mutex::new(loaded),
        warnings: Mutex::new(warnings),
        last_errors: Mutex::new(std::collections::HashMap::new()),
        restart_attempts: Mutex::new(std::collections::HashMap::new()),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(engine)
        .manage(DbState(Mutex::new(db_conn)))
        .manage(EphemeralStore::new())
        .invoke_handler(tauri::generate_handler![
            get_active_models,
            list_model_registry,
            load_model,
            unload_model,
            get_warnings,
            create_session,
            list_sessions,
            save_message,
            get_messages,
            delete_session,
            search_documents,
            get_session_documents,
            list_documents,
            pick_and_ingest_document,
            save_text_to_file,
            delete_document,
            add_note,
            list_notes,
            delete_note,
            update_note,
            start_disappearing_session,
            disappearing_save_message,
            end_disappearing_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Watch for the drive itself disappearing (unplugged pendrive) and
    // react within about a second: stop the engine, wipe any disappearing
    // session, and tell the frontend, rather than let writes fail into a
    // half-vanished file handle. See watchdog.rs for why this is a poll
    // rather than an instant OS notification.
    let watchdog_handle = app.handle().clone();
    let drive_lost = watchdog::spawn(root.clone(), move || {
        eprintln!("[offline-ai] warning: drive disconnected -- stopping the engine and blocking further writes");
        kill_engine(&watchdog_handle);
        watchdog_handle.state::<EphemeralStore>().end_all();
        let _ = watchdog_handle.emit("drive-removed", ());
    });
    app.manage(drive_lost);

    // Watch for an engine process dying on its own (crash, OOM kill, a
    // flag it doesn't like) and report it within a couple of seconds,
    // instead of only noticing the next time some other command happens
    // to call `reap_dead_engines`. This is what turns "the UI just says
    // Waiting for engine... forever" into an actual, visible error.
    let engine_watch_handle = app.handle().clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let state = engine_watch_handle.state::<EngineState>();
        reap_dead_engines(&state, Some(&engine_watch_handle));
    });

    app.run(|app_handle, event| {
        // Covers both the window being closed and the process being asked
        // to exit, so no backend process survives as an orphan.
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            kill_engine(app_handle);
            // Catch-all wipe: a disappearing session ended by closing the
            // window (rather than an explicit "end session" action) still
            // leaves nothing behind in memory.
            app_handle.state::<EphemeralStore>().end_all();
        }
    });
}
