//! MARK: - Single instance
//!
//! An advisory file lock held for the life of the process: a second copy started directly (not through
//! LaunchServices, which already refuses to open a second copy of a bundle) finds the lock taken and
//! quits instead of converting every text twice. The lock dies with the process, so a crash never
//! leaves the app locked out.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LockError {
    /// Another copy of the app holds the lock.
    AlreadyRunning,
    Io(io::Error),
}

/// Held while the app runs; dropping it (or exiting) releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(error)) => Err(LockError::Io(error)),
        }
    }
}

/// `<temp dir>/<bundle id>.lock`. The temp dir is per user, so two users can run the app side by side.
pub fn lock_path(bundle_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{bundle_id}.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_lock(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "reshift-instance-test-{}-{name}.lock",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn the_first_instance_gets_the_lock() {
        let path = temp_lock("first");
        assert!(InstanceLock::acquire(&path).is_ok());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_second_instance_is_refused_while_the_first_runs() {
        let path = temp_lock("second");
        let _first = InstanceLock::acquire(&path).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&path),
            Err(LockError::AlreadyRunning)
        ));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn the_lock_is_released_when_the_first_instance_ends() {
        let path = temp_lock("released");
        drop(InstanceLock::acquire(&path).unwrap());
        assert!(InstanceLock::acquire(&path).is_ok());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn different_lock_files_do_not_interfere() {
        let (a, b) = (temp_lock("a"), temp_lock("b"));
        let _first = InstanceLock::acquire(&a).unwrap();
        assert!(InstanceLock::acquire(&b).is_ok());
        let _ = (fs::remove_file(&a), fs::remove_file(&b));
    }

    #[test]
    fn an_unusable_path_is_an_io_error() {
        let path = std::env::temp_dir()
            .join("reshift-no-such-dir-xyz")
            .join("x.lock");
        assert!(matches!(
            InstanceLock::acquire(&path),
            Err(LockError::Io(_))
        ));
    }

    #[test]
    fn the_lock_path_is_named_after_the_bundle() {
        let path = lock_path("com.reshift.rust");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("com.reshift.rust.lock")
        );
        assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    }
}
