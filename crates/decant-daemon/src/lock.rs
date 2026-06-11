use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// An exclusive advisory lock proving this is the only running daemon.
/// Releases on drop (and the OS releases it if the process dies).
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.try_lock_exclusive().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "another decant daemon is running",
            )
        })?;
        // Best-effort PID record for humans; not used for locking.
        let _ = file.set_len(0);
        let _ = write!(file, "{}", std::process::id());
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_while_first_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let held = InstanceLock::acquire(&path).expect("first lock");
        let second = InstanceLock::acquire(&path);
        assert!(second.is_err(), "second lock must fail while first is held");

        drop(held);
        let third = InstanceLock::acquire(&path);
        assert!(third.is_ok(), "lock available again after release");
    }

    #[test]
    fn acquire_creates_missing_parent_dirs() {
        // The lock path's parent does not exist yet; acquire must create it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/daemon.lock");
        let held = InstanceLock::acquire(&path).expect("lock under fresh dirs");
        assert!(path.exists(), "lock file (and parents) created");
        drop(held);
    }

    #[test]
    fn acquire_propagates_parent_create_dir_failure() {
        // The lock path's parent is an existing *file*, so `create_dir_all`
        // fails and `acquire` surfaces the io error.
        let dir = tempfile::tempdir().unwrap();
        let file_as_parent = dir.path().join("not-a-dir");
        std::fs::write(&file_as_parent, "x").unwrap();
        let path = file_as_parent.join("daemon.lock");
        assert!(InstanceLock::acquire(&path).is_err());
    }
}
