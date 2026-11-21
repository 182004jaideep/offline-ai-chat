use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use zeroize::Zeroize;

/// "Disappearing mode" storage: chat history that lives only in this
/// process's memory for the lifetime of the session, and is never written
/// to the SQLite database or any file. Explicitly zeroized on session end
/// (or app exit) rather than just left to the allocator -- this is
/// best-effort hardening, not a guarantee against OS-level paging or
/// hibernation on the host, which the app has no control over.
#[derive(Default)]
struct EphemeralMessage {
    role: String,
    content: String,
}

impl EphemeralMessage {
    fn wipe(mut self) {
        self.role.zeroize();
        self.content.zeroize();
    }
}

static NEXT_TOKEN_SUFFIX: AtomicU64 = AtomicU64::new(0);

pub struct EphemeralStore {
    sessions: Mutex<HashMap<String, Vec<EphemeralMessage>>>,
}

impl EphemeralStore {
    pub fn new() -> Self {
        EphemeralStore {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn start_session(&self) -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let suffix = NEXT_TOKEN_SUFFIX.fetch_add(1, Ordering::SeqCst);
        let token = format!("eph-{:x}-{:x}", millis, suffix);
        if let Ok(mut guard) = self.sessions.lock() {
            guard.insert(token.clone(), Vec::new());
        }
        token
    }

    pub fn save_message(&self, token: &str, role: &str, content: &str) -> Result<(), String> {
        let mut guard = self.sessions.lock().map_err(|e| e.to_string())?;
        match guard.get_mut(token) {
            Some(messages) => {
                messages.push(EphemeralMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
                Ok(())
            }
            None => Err(format!("unknown disappearing session token: {}", token)),
        }
    }

    /// Removes and explicitly wipes a session's messages. Called both when
    /// the user ends/clears a disappearing conversation and, as a
    /// catch-all, when the app itself exits.
    pub fn end_session(&self, token: &str) {
        if let Ok(mut guard) = self.sessions.lock() {
            if let Some(messages) = guard.remove(token) {
                for m in messages {
                    m.wipe();
                }
            }
        }
    }

    /// Wipes every still-open disappearing session. Used on app exit so a
    /// window close (not an explicit "end session" click) still leaves
    /// nothing behind in memory.
    pub fn end_all(&self) {
        if let Ok(mut guard) = self.sessions.lock() {
            for (_, messages) in guard.drain() {
                for m in messages {
                    m.wipe();
                }
            }
        }
    }

    #[cfg(test)]
    fn message_count(&self, token: &str) -> Option<usize> {
        self.sessions
            .lock()
            .ok()
            .and_then(|g| g.get(token).map(|m| m.len()))
    }

    #[cfg(test)]
    fn session_count(&self) -> usize {
        self.sessions.lock().map(|g| g.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique() {
        let store = EphemeralStore::new();
        let a = store.start_session();
        let b = store.start_session();
        assert_ne!(a, b);
        assert_eq!(store.session_count(), 2);
    }

    #[test]
    fn save_message_requires_known_token() {
        let store = EphemeralStore::new();
        let token = store.start_session();
        assert!(store.save_message(&token, "user", "hello").is_ok());
        assert_eq!(store.message_count(&token), Some(1));
        assert!(store.save_message("not-a-real-token", "user", "hi").is_err());
    }

    #[test]
    fn end_session_removes_it_without_touching_others() {
        let store = EphemeralStore::new();
        let a = store.start_session();
        let b = store.start_session();
        store.save_message(&a, "user", "secret").unwrap();
        store.save_message(&b, "user", "unrelated").unwrap();

        store.end_session(&a);

        assert_eq!(store.message_count(&a), None, "ended session must be gone");
        assert_eq!(
            store.message_count(&b),
            Some(1),
            "other sessions must be untouched"
        );
    }

    #[test]
    fn end_all_clears_everything() {
        let store = EphemeralStore::new();
        let a = store.start_session();
        let b = store.start_session();
        store.save_message(&a, "user", "one").unwrap();
        store.save_message(&b, "user", "two").unwrap();

        store.end_all();

        assert_eq!(store.session_count(), 0);
    }
}
