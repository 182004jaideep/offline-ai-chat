use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::format_size;
use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;

pub fn db_path(root: &Path) -> PathBuf {
    root.join("data").join("offlineai.db")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Moves a damaged database file aside (timestamped, never overwritten)
/// instead of deleting or leaving it in place to break every future
/// launch. Its WAL/SHM sidecar files belong to the same broken file and go
/// with it.
fn quarantine_corrupt_db(path: &Path) {
    let millis = now_millis();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "offlineai".to_string());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let quarantined = parent.join(format!("{}.corrupt-{}.db", stem, millis));
    let _ = std::fs::rename(path, &quarantined);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

/// Opens (creating if needed) the drive-local SQLite database and brings
/// its schema up to date. WAL mode + NORMAL synchronous keep write
/// transactions short-lived (one per message, not per session), which
/// matters because this file lives on removable media that can be
/// unplugged at any time. Returns any recovery warning alongside the
/// connection so the caller can surface it rather than fail silently.
pub fn open(root: &Path) -> rusqlite::Result<(Connection, Option<String>)> {
    let path = db_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut warning = None;
    if path.exists() {
        let healthy = Connection::open(&path)
            .and_then(|conn| conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))
            .map(|result| result == "ok")
            .unwrap_or(false);
        if !healthy {
            quarantine_corrupt_db(&path);
            warning = Some(
                "The chat/document database looked damaged (likely from an earlier unsafe drive removal) and was moved aside; starting fresh. The old file is still on the drive, just renamed."
                    .to_string(),
            );
        }
    }

    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;",
    )?;
    migrate(&conn)?;
    Ok((conn, warning))
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);

        CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            source_path TEXT NOT NULL,
            added_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS document_chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS document_chunks_fts USING fts5(
            content,
            content='document_chunks',
            content_rowid='id'
        );
        CREATE TRIGGER IF NOT EXISTS document_chunks_ai AFTER INSERT ON document_chunks BEGIN
            INSERT INTO document_chunks_fts(rowid, content) VALUES (new.id, new.content);
        END;
        CREATE TRIGGER IF NOT EXISTS document_chunks_ad AFTER DELETE ON document_chunks BEGIN
            INSERT INTO document_chunks_fts(document_chunks_fts, rowid, content) VALUES('delete', old.id, old.content);
        END;

        CREATE TABLE IF NOT EXISTS notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS session_documents (
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            PRIMARY KEY (session_id, document_id)
        );",
    )
}

// --- sessions / messages -----------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: i64,
    pub title: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

pub fn create_session(conn: &Connection, title: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO sessions (title, created_at) VALUES (?1, ?2)",
        (title, now_millis()),
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_sessions(conn: &Connection) -> rusqlite::Result<Vec<SessionRow>> {
    let mut stmt =
        conn.prepare("SELECT id, title, created_at FROM sessions ORDER BY created_at DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(SessionRow {
            id: r.get(0)?,
            title: r.get(1)?,
            created_at: r.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn save_message(
    conn: &Connection,
    session_id: i64,
    role: &str,
    content: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        (session_id, role, content, now_millis()),
    )?;
    Ok(())
}

pub fn get_messages(conn: &Connection, session_id: i64) -> rusqlite::Result<Vec<MessageRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, role, content, created_at FROM messages WHERE session_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([session_id], |r| {
        Ok(MessageRow {
            id: r.get(0)?,
            role: r.get(1)?,
            content: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_session(conn: &Connection, session_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
    Ok(())
}

// --- documents / offline search -----------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRow {
    pub id: i64,
    pub name: String,
    pub source_path: String,
    pub added_at: i64,
    pub chunk_count: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub document_name: String,
    pub content: String,
}

/// Splits text on blank lines into paragraphs, then greedily packs
/// paragraphs into chunks up to `max_chars` -- simple, dependency-free,
/// and good enough for keyword (FTS5) retrieval at personal-drive scale.
fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if !current.is_empty() && current.len() + para.len() + 2 > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Pulls plain text out of a .docx by reading the zip's document.xml
/// directly and extracting `<w:t>` text runs, grouped into paragraphs by
/// `</w:p>` boundaries. Lighter than a full docx parser and all this needs
/// -- formatting, tables-as-structure, images, etc. are all dropped.
fn extract_docx_text(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| format!("could not open {:?}: {}", path, e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("not a valid .docx file {:?}: {}", path, e))?;
    let mut xml = String::new();
    {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|e| format!("could not find document content inside {:?}: {}", path, e))?;
        entry
            .read_to_string(&mut xml)
            .map_err(|e| format!("could not read document content: {}", e))?;
    }

    let text_re = Regex::new(r"<w:t[^>]*>(.*?)</w:t>").map_err(|e| e.to_string())?;
    let mut paragraphs = Vec::new();
    for para_xml in xml.split("</w:p>") {
        let mut para_text = String::new();
        for cap in text_re.captures_iter(para_xml) {
            para_text.push_str(&unescape_xml(&cap[1]));
        }
        let para_text = para_text.trim();
        if !para_text.is_empty() {
            paragraphs.push(para_text.to_string());
        }
    }
    Ok(paragraphs.join("\n\n"))
}

/// Flattens every sheet of a spreadsheet (.xlsx/.xls/.xlsm/.ods) into
/// tab-separated rows, one sheet section at a time -- enough for keyword
/// search even though it throws away formulas and formatting.
fn extract_spreadsheet_text(path: &Path) -> Result<String, String> {
    use calamine::{open_workbook_auto, Reader};

    let mut workbook = open_workbook_auto(path)
        .map_err(|e| format!("could not open spreadsheet {:?}: {}", path, e))?;
    let mut out = String::new();
    for sheet_name in workbook.sheet_names().to_owned() {
        let Ok(range) = workbook.worksheet_range(&sheet_name) else {
            continue;
        };
        out.push_str(&format!("# Sheet: {}\n\n", sheet_name));
        for row in range.rows() {
            let line = row
                .iter()
                .map(|cell| cell.to_string())
                .collect::<Vec<_>>()
                .join("\t");
            if !line.trim().is_empty() {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out.push('\n');
    }
    Ok(out)
}

/// 2 MB is generous for source files and small text docs while keeping a
/// single huge ingest from blowing up the FTS index or stalling the UI.
/// Applies to every plain-text-ish format (txt/md and now source code),
/// closing a gap that existed even before source files were added -- the
/// old txt/md branch had no cap at all.
const MAX_TEXT_INGEST_BYTES: u64 = 2 * 1024 * 1024;

fn read_text_with_cap(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("could not read {:?}: {}", path, e))?;
    if meta.len() > MAX_TEXT_INGEST_BYTES {
        return Err(format!(
            "{:?} is {} -- over the {} limit for text ingestion. Trim it or ingest an excerpt instead.",
            path,
            format_size(meta.len()),
            format_size(MAX_TEXT_INGEST_BYTES)
        ));
    }
    std::fs::read_to_string(path).map_err(|e| format!("could not read {:?}: {}", path, e))
}

fn extract_text(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "txt" | "md" | "markdown"
        // source code
        | "py" | "js" | "jsx" | "ts" | "tsx" | "rs" | "go" | "java" | "c" | "h" | "cpp" | "hpp"
        | "cs" | "rb" | "php" | "swift" | "kt" | "lua" | "r" | "vue" | "svelte"
        // config / data / markup commonly read alongside code
        | "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "sh" | "bash" | "sql"
        | "css" | "scss" | "html" | "xml" => read_text_with_cap(path),
        "pdf" => pdf_extract::extract_text(path)
            .map_err(|e| format!("could not extract text from PDF {:?}: {}", path, e)),
        "docx" => extract_docx_text(path),
        "xlsx" | "xls" | "xlsm" | "ods" => extract_spreadsheet_text(path),
        other => Err(format!(
            "unsupported file type \".{}\" for {:?} -- supported: text/code files, .pdf, .docx, .xlsx/.xls/.ods",
            other, path
        )),
    }
}

/// Extracts text (format-appropriate), chunks it, and indexes it for
/// offline keyword search.
const CODE_EXTENSIONS: &[&str] = &[
    "py", "js", "jsx", "ts", "tsx", "rs", "go", "java", "c", "h", "cpp", "hpp", "cs", "rb", "php",
    "swift", "kt", "lua", "r", "vue", "svelte", "json", "yaml", "yml", "toml", "ini", "cfg",
    "conf", "sh", "bash", "sql", "css", "scss", "html", "xml",
];

pub fn ingest_document(conn: &Connection, path: &Path) -> Result<(i64, usize), String> {
    let text = extract_text(path)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    conn.execute(
        "INSERT INTO documents (name, source_path, added_at) VALUES (?1, ?2, ?3)",
        (&name, path.to_string_lossy().as_ref(), now_millis()),
    )
    .map_err(|e| e.to_string())?;
    let document_id = conn.last_insert_rowid();

    // Code/config files keep more surrounding context per chunk than prose --
    // a function or block split across chunk boundaries is harder to use than
    // slightly larger chunks would be.
    let max_chars = if CODE_EXTENSIONS.contains(&ext.as_str()) {
        2400
    } else {
        1200
    };
    let chunks = chunk_text(&text, max_chars);
    for (i, chunk) in chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO document_chunks (document_id, chunk_index, content) VALUES (?1, ?2, ?3)",
            (document_id, i as i64, chunk),
        )
        .map_err(|e| e.to_string())?;
    }

    Ok((document_id, chunks.len()))
}

/// Links a document to a session, so retrieval can be scoped to "what was
/// attached in this conversation" rather than the whole library. Only
/// meaningful for Normal-mode sessions (Disappearing-mode conversations
/// have no row here to link to by design -- that scoping is tracked
/// client-side instead, so an ephemeral chat never leaves a trace of which
/// documents it touched in permanent storage).
pub fn link_document_to_session(
    conn: &Connection,
    session_id: i64,
    document_id: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO session_documents (session_id, document_id) VALUES (?1, ?2)",
        (session_id, document_id),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_session_document_ids(conn: &Connection, session_id: i64) -> Result<Vec<i64>, String> {
    let mut stmt = conn
        .prepare("SELECT document_id FROM session_documents WHERE session_id = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([session_id], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn delete_document(conn: &Connection, document_id: i64) -> rusqlite::Result<()> {
    // document_chunks has ON DELETE CASCADE on document_id, and the FTS
    // delete trigger keeps the search index in sync -- one statement is
    // enough.
    conn.execute("DELETE FROM documents WHERE id = ?1", [document_id])?;
    Ok(())
}

pub fn list_documents(conn: &Connection) -> rusqlite::Result<Vec<DocumentRow>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.name, d.source_path, d.added_at, COUNT(c.id)
         FROM documents d LEFT JOIN document_chunks c ON c.document_id = d.id
         GROUP BY d.id ORDER BY d.added_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DocumentRow {
            id: r.get(0)?,
            name: r.get(1)?,
            source_path: r.get(2)?,
            added_at: r.get(3)?,
            chunk_count: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Common English function words, dropped before building the search
/// query so they don't inflate the OR'd match set -- "the"/"is"/"this"
/// appear in nearly every chunk, so without this every query effectively
/// matched almost anything. No length-based filter alongside this: a
/// short-but-meaningful code identifier ("fn", "id") would wrongly get
/// dropped too, and this app searches source code now, not just prose.
const FTS_STOPWORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "this", "that",
    "these", "those", "and", "or", "but", "if", "then", "to", "of", "in", "on", "at", "for",
    "with", "as", "by", "from", "it", "its", "i", "you", "your", "my", "me", "do", "does",
    "did", "what", "how", "can", "could", "would", "should", "will", "shall", "not", "so",
];

/// Turns a free-form query into a safe FTS5 MATCH expression: every
/// non-stopword OR'd together (quoted so stray punctuation in a chat
/// prompt can't produce an FTS5 syntax error).
fn sanitize_fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .filter(|w| !FTS_STOPWORDS.contains(&w.to_lowercase().as_str()))
        .map(|w| format!("\"{}\"", w))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// `document_ids: None` searches the whole library (original behavior).
/// `Some(ids)` restricts to exactly those documents -- used to scope
/// retrieval to "what was attached in this conversation" rather than
/// everything ever ingested.
pub fn search_documents(
    conn: &Connection,
    query: &str,
    limit: i64,
    document_ids: Option<&[i64]>,
) -> rusqlite::Result<Vec<SearchResult>> {
    let Some(fts_query) = sanitize_fts_query(query) else {
        return Ok(Vec::new());
    };

    let to_result = |r: &rusqlite::Row| {
        Ok(SearchResult {
            document_name: r.get(0)?,
            content: r.get(1)?,
        })
    };

    match document_ids {
        None => {
            let mut stmt = conn.prepare(
                "SELECT d.name, dc.content
                 FROM document_chunks_fts f
                 JOIN document_chunks dc ON dc.id = f.rowid
                 JOIN documents d ON d.id = dc.document_id
                 WHERE document_chunks_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map((fts_query, limit), to_result)?;
            rows.collect()
        }
        Some(ids) if ids.is_empty() => Ok(Vec::new()),
        Some(ids) => {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT d.name, dc.content
                 FROM document_chunks_fts f
                 JOIN document_chunks dc ON dc.id = f.rowid
                 JOIN documents d ON d.id = dc.document_id
                 WHERE document_chunks_fts MATCH ? AND dc.document_id IN ({})
                 ORDER BY rank
                 LIMIT ?",
                placeholders
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 2);
            params.push(&fts_query);
            for id in ids {
                params.push(id);
            }
            params.push(&limit);
            let rows = stmt.query_map(params.as_slice(), to_result)?;
            rows.collect()
        }
    }
}

// --- notes / tracked data -----------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NoteRow {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn add_note(conn: &Connection, title: &str, body: &str) -> rusqlite::Result<i64> {
    let now = now_millis();
    conn.execute(
        "INSERT INTO notes (title, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        (title, body, now),
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_notes(conn: &Connection) -> rusqlite::Result<Vec<NoteRow>> {
    let mut stmt = conn
        .prepare("SELECT id, title, body, created_at, updated_at FROM notes ORDER BY updated_at DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(NoteRow {
            id: r.get(0)?,
            title: r.get(1)?,
            body: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn delete_note(conn: &Connection, note_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", [note_id])?;
    Ok(())
}

pub fn update_note(conn: &Connection, note_id: i64, title: &str, body: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE notes SET title = ?1, body = ?2, updated_at = ?3 WHERE id = ?4",
        (title, body, now_millis(), note_id),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn delete_document_cascades_chunks_and_fts() {
        let conn = test_conn();
        let dir = std::env::temp_dir().join(format!("offlineai-test-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "First paragraph.\n\nSecond paragraph with content.").unwrap();

        let (_, chunks) = ingest_document(&conn, &path).unwrap();
        assert!(chunks > 0);

        let doc_id: i64 = conn
            .query_row("SELECT id FROM documents LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let chunk_count_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_chunks", [], |r| r.get(0))
            .unwrap();
        assert!(chunk_count_before > 0);

        delete_document(&conn, doc_id).unwrap();

        let doc_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap();
        let chunk_count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_chunks", [], |r| r.get(0))
            .unwrap();
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM document_chunks_fts", [], |r| r.get(0))
            .unwrap();

        assert_eq!(doc_count, 0, "document row must be gone");
        assert_eq!(chunk_count_after, 0, "chunks must cascade-delete");
        assert_eq!(fts_count, 0, "FTS index must be cleaned up via the delete trigger");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_documents_scopes_by_session() {
        let conn = test_conn();
        let session_id = create_session(&conn, "test session").unwrap();

        // Distinct prefix from the other test's temp dir -- both tests can
        // run concurrently and `now_millis()` alone isn't unique enough to
        // avoid a same-millisecond collision between them.
        let dir = std::env::temp_dir().join(format!("offlineai-scope-test-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        std::fs::write(&path_a, "apple banana cherry unique_a_term").unwrap();
        std::fs::write(&path_b, "apple banana cherry unique_b_term").unwrap();

        let (doc_a, _) = ingest_document(&conn, &path_a).unwrap();
        let (doc_b, _) = ingest_document(&conn, &path_b).unwrap();

        // Only doc_a is attached to this session.
        link_document_to_session(&conn, session_id, doc_a).unwrap();
        let linked_ids = get_session_document_ids(&conn, session_id).unwrap();
        assert_eq!(linked_ids, vec![doc_a]);

        // Both documents share "apple", but scoped search must only see
        // the one actually linked to this session.
        let scoped = search_documents(&conn, "apple", 10, Some(&linked_ids)).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].document_name, "a");

        let unscoped = search_documents(&conn, "apple", 10, None).unwrap();
        assert_eq!(unscoped.len(), 2, "unscoped search must still see both");

        let _ = doc_b; // used only to prove it's excluded when scoped
        let _ = std::fs::remove_dir_all(&dir);
    }
}
