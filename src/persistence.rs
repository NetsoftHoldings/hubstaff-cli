use crate::error::CliError;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o600))?;
    }

    temp.persist(path).map_err(|e| CliError::from(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_SEQ: AtomicU32 = AtomicU32::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = TEST_SEQ.fetch_add(1, Ordering::SeqCst);
            let pid = std::process::id();
            let path =
                std::env::temp_dir().join(format!("hubstaff-persistence-{pid}-{id}-{label}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn write_atomic_creates_file_with_contents() {
        let dir = TestDir::new("create");
        let path = dir.path().join("data.bin");
        write_atomic(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn write_atomic_overwrites_existing_file() {
        let dir = TestDir::new("overwrite");
        let path = dir.path().join("data.bin");
        fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn write_atomic_removes_temp_file() {
        let dir = TestDir::new("tempfile");
        let path = dir.path().join("data.bin");
        write_atomic(&path, b"x").unwrap();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        assert_eq!(entries, vec![path]);
    }

    #[test]
    fn write_atomic_concurrent_writes_do_not_collide() {
        let dir = TestDir::new("concurrent");
        let path = dir.path().join("data.bin");
        let p1 = path.clone();
        let p2 = path.clone();
        let h1 = std::thread::spawn(move || {
            for _ in 0..50 {
                write_atomic(&p1, b"AAAA").unwrap();
            }
        });
        let h2 = std::thread::spawn(move || {
            for _ in 0..50 {
                write_atomic(&p2, b"BBBB").unwrap();
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();
        let final_bytes = fs::read(&path).unwrap();
        assert!(final_bytes == b"AAAA" || final_bytes == b"BBBB");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_sets_0o600_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new("perms");
        let path = dir.path().join("data.bin");
        write_atomic(&path, b"x").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
