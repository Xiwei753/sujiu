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
    fn save(&self, key: &str, contents: &str);

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

    fn save(&self, key: &str, contents: &str) {
        self.documents
            .lock()
            .unwrap()
            .insert(key.to_owned(), contents.to_owned());
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

    fn document(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }
}

impl AppStorage for FileStorage {
    fn load(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.document(key)).ok()
    }

    fn save(&self, key: &str, contents: &str) {
        let target = self.document(key);
        // A key can name a document inside a directory — a conversation is
        // stored under its own id, not beside the others — so the parent has to
        // exist before the write does. Creating it here rather than at load time
        // is what lets a first save create the whole tree.
        if let Some(parent) = target.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }

        let temporary = target.with_extension("tmp");
        if std::fs::write(&temporary, contents).is_err() {
            return;
        }
        let _ = std::fs::rename(&temporary, &target);
    }

    fn location(&self) -> Option<String> {
        Some(self.root.display().to_string())
    }
}
