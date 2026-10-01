//! Where the runtime keeps application data between launches.
//!
//! The runtime does not choose a location. A platform hands it one through a
//! platform service, and the runtime only knows how to read and write a
//! document there. That keeps filesystem policy on the platform side and keeps
//! the runtime usable in tests with no filesystem at all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One named document. `None` means the document does not exist yet.
pub trait AppStorage: Send + Sync {
    fn load(&self, key: &str) -> Option<String>;

    /// Write a document, or report why it could not be written.
    ///
    /// A failure has to be a failure. The runtime writes a manifest that names
    /// every other document, and a write that reports nothing leaves that
    /// manifest claiming a document was saved when it was not — the store then
    /// looks complete and is not. Callers that continue after an error have to
    /// be able to say so, which they cannot while the error is `()`.
    fn save(&self, key: &str, contents: &str) -> std::io::Result<()>;

    /// Remove whatever this key names, and succeed if there was nothing there.
    ///
    /// Used to retire a superseded copy of the store once the new one is safely
    /// in place. A key may name a directory, so an implementation that can only
    /// remove files is not enough.
    fn remove(&self, key: &str) -> std::io::Result<()>;

    /// Where the documents live, when the storage has a location. A runtime
    /// reports this so a frontend can show or diagnose its own storage.
    fn location(&self) -> Option<String> {
        None
    }
}

/// Storage that keeps documents for the lifetime of the process only. This is
/// the default, so a runtime without a platform storage still works and simply
/// forgets everything when it exits.
#[derive(Default)]
pub struct MemoryStorage {
    documents: Mutex<BTreeMap<String, String>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AppStorage for MemoryStorage {
    fn load(&self, key: &str) -> Option<String> {
        self.documents
            .lock()
            .unwrap()
            .get(key)
            .map(String::as_str)
            .map(str::to_owned)
    }

    fn save(&self, key: &str, contents: &str) -> std::io::Result<()> {
        self.documents
            .lock()
            .unwrap()
            .insert(key.to_owned(), contents.to_owned());
        Ok(())
    }

    fn remove(&self, key: &str) -> std::io::Result<()> {
        self.documents.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Storage in a directory the platform chose, for example the app's data
/// directory.
///
/// Writes go to a temporary file first and are then renamed, so a write that is
/// interrupted leaves the previous document intact rather than a truncated one.
pub struct FileStorage {
    root: PathBuf,
}

impl FileStorage {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a key resolves.
    ///
    /// A key is a **storage key**, not a path: it may name a document inside a
    /// directory, and its separators are chosen by the caller. The caller is
    /// `documents`, which maps every domain id through `documents::path_segment`
    /// before building a key, so an id cannot pick its own directory. This
    /// deliberately does **not** encode a key again — one mapping, applied once,
    /// on both the read and the write side, is what lets `load` find what `save`
    /// wrote. Encoding here as well would make the two disagree.
    fn document(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }
}

impl AppStorage for FileStorage {
    fn load(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.document(key)).ok()
    }

    fn save(&self, key: &str, contents: &str) -> std::io::Result<()> {
        let target = self.document(key);
        // A key can name a document inside a directory — a conversation is
        // stored under its own id, not beside the others — so the parent has to
        // exist before the write does. Creating it here rather than at load time
        // is what lets a first save create the whole tree.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("could not create {}: {error}", parent.display()),
                )
            })?;
        }

        let temporary = target.with_extension("tmp");
        std::fs::write(&temporary, contents).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("could not write {}: {error}", temporary.display()),
            )
        })?;
        // The rename is what makes a write atomic, so its failure is the one that
        // must not be swallowed: the document it was going to replace is still
        // the old one, and a caller that believes otherwise will record a
        // generation as saved when it is not.
        std::fs::rename(&temporary, &target).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("could not publish {}: {error}", target.display()),
            )
        })
    }

    fn remove(&self, key: &str) -> std::io::Result<()> {
        let target = self.document(key);
        if target.is_dir() {
            return std::fs::remove_dir_all(&target).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("could not remove {}: {error}", target.display()),
                )
            });
        }

        match std::fs::remove_file(&target) {
            Ok(()) => Ok(()),
            // A key that was never there is already in the state the caller
            // asked for.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(std::io::Error::new(
                error.kind(),
                format!("could not remove {}: {error}", target.display()),
            )),
        }
    }

    fn location(&self) -> Option<String> {
        Some(self.root.display().to_string())
    }
}
