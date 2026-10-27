//! Files by path. On the desktop these are the files on disk. The browser
//! has no file system: there the files the user picks and the files the
//! app writes are kept in memory for the session, under made-up paths.

use std::io;
use std::path::Path;

pub use imp::*;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;

    pub type File = std::fs::File;

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    pub fn open(path: &Path) -> io::Result<File> {
        std::fs::File::open(path)
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    static FILES: Mutex<BTreeMap<PathBuf, Arc<[u8]>>> = Mutex::new(BTreeMap::new());

    pub type File = io::Cursor<Arc<[u8]>>;

    fn get(path: &Path) -> io::Result<Arc<[u8]>> {
        FILES
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file not found"))
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        get(path).map(|b| b.to_vec())
    }

    pub fn open(path: &Path) -> io::Result<File> {
        get(path).map(io::Cursor::new)
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        FILES
            .lock()
            .unwrap()
            .insert(path.to_owned(), Arc::from(bytes));
        Ok(())
    }

    pub fn exists(path: &Path) -> bool {
        FILES.lock().unwrap().contains_key(path)
    }

    /// Forgets a file (once it is downloaded, say).
    pub fn remove(path: &Path) {
        FILES.lock().unwrap().remove(path);
    }

    /// The files in `dir` and below.
    pub fn list(dir: &Path) -> Vec<PathBuf> {
        FILES
            .lock()
            .unwrap()
            .keys()
            .filter(|p| p.starts_with(dir))
            .cloned()
            .collect()
    }
}
