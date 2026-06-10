//! Live-session detection: which source files are being written *right now*.
//!
//! The filesystem watcher reports every relevant write here. A session is
//! "active" while its file keeps changing within [`ACTIVE_WINDOW`]; a periodic
//! sweep (piggybacking the SSE keep-alive tick) expires quiet entries. State is
//! in-memory only — it describes "now", so losing it on restart is correct.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long after the last write a session still counts as active. Claude/Codex
/// append continuously during a turn; two minutes of silence means the user
/// walked away or the turn ended.
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(120);

/// Which tool a watched source path belongs to, by directory ancestry and the
/// same filename rules ingest uses for discovery.
pub fn classify_source_path(
    path: &Path,
    claude_dir: &Path,
    codex_dir: &Path,
) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".jsonl") {
        return None;
    }
    if path.starts_with(claude_dir) {
        return Some("claude_code");
    }
    if path.starts_with(codex_dir) && name.starts_with("rollout-") {
        return Some("codex");
    }
    None
}

#[derive(Debug)]
struct Entry {
    last_write: Instant,
    tool: &'static str,
}

/// Tracks per-source-path write recency. Shared between the watcher callback
/// (writes), the sweep task (expiry), and `/analytics/now` (reads).
#[derive(Debug, Default)]
pub struct ActivityTracker {
    inner: Mutex<HashMap<PathBuf, Entry>>,
}

/// A currently-active source file, for `/analytics/now`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSource {
    pub path: PathBuf,
    pub tool: &'static str,
    pub idle: Duration,
}

impl ActivityTracker {
    /// Record a write at `now`. Returns `true` when the path transitioned
    /// idle→active (first sighting, or first write after expiry) — the caller
    /// broadcasts a `session_activity{state:"active"}` event exactly then, so
    /// a busy session doesn't spam the stream.
    pub fn record_write(&self, path: &Path, tool: &'static str, now: Instant) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.get_mut(path) {
            Some(e) => {
                let was_idle = now.duration_since(e.last_write) > ACTIVE_WINDOW;
                e.last_write = now;
                e.tool = tool;
                was_idle
            }
            None => {
                inner.insert(
                    path.to_path_buf(),
                    Entry {
                        last_write: now,
                        tool,
                    },
                );
                true
            }
        }
    }

    /// Remove entries quiet for longer than [`ACTIVE_WINDOW`] and return them;
    /// the caller broadcasts `state:"idle"` per expired path. Each expiry is
    /// reported exactly once (the entry is gone afterwards).
    pub fn sweep(&self, now: Instant) -> Vec<(PathBuf, &'static str)> {
        let mut inner = self.inner.lock().unwrap();
        let expired: Vec<PathBuf> = inner
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_write) > ACTIVE_WINDOW)
            .map(|(p, _)| p.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|p| inner.remove_entry(&p).map(|(p, e)| (p, e.tool)))
            .collect()
    }

    /// Currently-active sources, most recently written first.
    pub fn active(&self, now: Instant) -> Vec<ActiveSource> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<ActiveSource> = inner
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_write) <= ACTIVE_WINDOW)
            .map(|(p, e)| ActiveSource {
                path: p.clone(),
                tool: e.tool,
                idle: now.duration_since(e.last_write),
            })
            .collect();
        out.sort_by_key(|a| a.idle);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_write_transitions_to_active_and_repeat_writes_do_not() {
        let t = ActivityTracker::default();
        let t0 = Instant::now();
        let p = Path::new("/x/sess.jsonl");
        assert!(
            t.record_write(p, "claude_code", t0),
            "first sighting announces"
        );
        assert!(
            !t.record_write(p, "claude_code", t0 + Duration::from_secs(5)),
            "writes within the window stay silent"
        );
        assert_eq!(t.active(t0 + Duration::from_secs(6)).len(), 1);
    }

    #[test]
    fn write_after_expiry_reannounces() {
        let t = ActivityTracker::default();
        let t0 = Instant::now();
        let p = Path::new("/x/sess.jsonl");
        t.record_write(p, "claude_code", t0);
        let later = t0 + ACTIVE_WINDOW + Duration::from_secs(1);
        assert!(
            t.record_write(p, "claude_code", later),
            "a write after going quiet is a fresh activation"
        );
    }

    #[test]
    fn sweep_expires_once_and_active_excludes_expired() {
        let t = ActivityTracker::default();
        let t0 = Instant::now();
        t.record_write(Path::new("/x/a.jsonl"), "claude_code", t0);
        t.record_write(
            Path::new("/x/b.jsonl"),
            "codex",
            t0 + Duration::from_secs(60),
        );

        let at = t0 + ACTIVE_WINDOW + Duration::from_secs(30);
        assert_eq!(t.active(at).len(), 1, "only b is still inside the window");
        let expired = t.sweep(at);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, PathBuf::from("/x/a.jsonl"));
        assert!(t.sweep(at).is_empty(), "expiry reported exactly once");
    }

    #[test]
    fn classify_source_path_by_dir_and_name() {
        let claude = Path::new("/h/.claude/projects");
        let codex = Path::new("/h/.codex");
        assert_eq!(
            classify_source_path(Path::new("/h/.claude/projects/p/s.jsonl"), claude, codex),
            Some("claude_code")
        );
        assert_eq!(
            classify_source_path(
                Path::new("/h/.codex/sessions/2026/rollout-1.jsonl"),
                claude,
                codex
            ),
            Some("codex")
        );
        // codex non-rollout and non-jsonl files don't count
        assert_eq!(
            classify_source_path(Path::new("/h/.codex/sessions/index.jsonl"), claude, codex),
            None
        );
        assert_eq!(
            classify_source_path(Path::new("/h/.claude/projects/p/s.tmp"), claude, codex),
            None
        );
        assert_eq!(
            classify_source_path(Path::new("/elsewhere/s.jsonl"), claude, codex),
            None
        );
    }
}
