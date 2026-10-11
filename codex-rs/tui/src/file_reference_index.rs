use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::RwLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

static FILE_REFERENCE_INDEX: LazyLock<FileReferenceIndex> =
    LazyLock::new(FileReferenceIndex::default);

#[derive(Default)]
struct FileReferenceIndex {
    next_generation: AtomicU64,
    roots: RwLock<HashMap<PathBuf, RootIndexState>>,
}

#[derive(Debug)]
struct RootIndexState {
    generation: u64,
    basenames: Option<HashMap<String, BasenameResolution>>,
}

#[derive(Debug)]
enum BasenameResolution {
    Unique(PathBuf),
    Ambiguous,
}

pub(crate) fn refresh(root: PathBuf) {
    FILE_REFERENCE_INDEX.refresh(root);
}

pub(crate) fn resolve_unique_basename(root: &Path, basename: &str) -> Option<PathBuf> {
    FILE_REFERENCE_INDEX.resolve_unique_basename(root, basename)
}

#[cfg(test)]
pub(crate) fn refresh_blocking(root: PathBuf) {
    FILE_REFERENCE_INDEX.refresh_blocking(root);
}

impl FileReferenceIndex {
    fn refresh(&self, root: PathBuf) {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let Ok(mut roots) = self.roots.write() else {
                return;
            };
            match roots.entry(root.clone()) {
                Entry::Occupied(_) => return,
                Entry::Vacant(entry) => {
                    entry.insert(RootIndexState {
                        generation,
                        basenames: None,
                    });
                }
            }
        }

        std::thread::spawn(move || {
            let basenames = build_basename_index(root.as_path());
            FILE_REFERENCE_INDEX.finish_refresh(root, generation, basenames);
        });
    }

    fn finish_refresh(
        &self,
        root: PathBuf,
        generation: u64,
        basenames: HashMap<String, BasenameResolution>,
    ) {
        let Ok(mut roots) = self.roots.write() else {
            return;
        };
        let Some(root_state) = roots.get_mut(&root) else {
            return;
        };
        if root_state.generation != generation || root_state.basenames.is_some() {
            return;
        }
        root_state.basenames = Some(basenames);
    }

    fn resolve_unique_basename(&self, root: &Path, basename: &str) -> Option<PathBuf> {
        let Ok(roots) = self.roots.read() else {
            return None;
        };
        let root_state = roots.get(root)?;
        let basenames = root_state.basenames.as_ref()?;
        match basenames.get(basename)? {
            BasenameResolution::Unique(path) => Some(path.clone()),
            BasenameResolution::Ambiguous => None,
        }
    }

    #[cfg(test)]
    fn refresh_blocking(&self, root: PathBuf) {
        let basenames = build_basename_index(root.as_path());
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let Ok(mut roots) = self.roots.write() else {
            return;
        };
        roots.insert(
            root,
            RootIndexState {
                generation,
                basenames: Some(basenames),
            },
        );
    }
}

fn build_basename_index(root: &Path) -> HashMap<String, BasenameResolution> {
    let mut basenames = HashMap::new();
    index_dir(root, &mut basenames);
    basenames
}

fn index_dir(dir: &Path, basenames: &mut HashMap<String, BasenameResolution>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(name, ".git" | "target" | "node_modules") {
                continue;
            }
            index_dir(path.as_path(), basenames);
            continue;
        }

        if !file_type.is_file() {
            continue;
        }
        let Some(basename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        match basenames.entry(basename.to_string()) {
            Entry::Occupied(mut existing) => {
                existing.insert(BasenameResolution::Ambiguous);
            }
            Entry::Vacant(slot) => {
                slot.insert(BasenameResolution::Unique(path));
            }
        }
    }
}
