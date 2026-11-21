use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(750);

/// Shared flag other parts of the app check before touching the drive
/// (e.g. before a SQLite write), so a lost drive fails fast and cleanly
/// instead of hanging or producing a confusing OS error partway through.
#[derive(Clone)]
pub struct DriveLost(pub Arc<AtomicBool>);

impl DriveLost {
    pub fn is_lost(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Spawns a background thread that polls whether `root` is still
/// reachable, and runs `on_lost` once, the first time it isn't.
///
/// Removable media can vanish with zero warning, and there is no portable,
/// dependency-free way to get an instant OS push notification for that --
/// true instant detection needs an unsafe, Windows-specific window-message
/// hook (WM_DEVICECHANGE) that this app does not install. Polling on a
/// short interval is the pragmatic trade-off: it reacts within roughly a
/// second instead of instantly, in exchange for a small, safe, portable
/// implementation. This is disclosed, not hidden -- see the Phase 5 notes.
pub fn spawn(root: PathBuf, on_lost: impl Fn() + Send + 'static) -> DriveLost {
    let lost = Arc::new(AtomicBool::new(false));
    let flag = DriveLost(lost.clone());

    thread::spawn(move || loop {
        thread::sleep(POLL_INTERVAL);
        if lost.load(Ordering::SeqCst) {
            // Already reported. Nothing more to do -- the app does not
            // attempt automatic resume if the drive comes back, since the
            // engine process and open file handles are already gone by
            // then; a restart is the clean path back.
            continue;
        }
        if std::fs::metadata(&root).is_err() {
            lost.store(true, Ordering::SeqCst);
            on_lost();
        }
    });

    flag
}
