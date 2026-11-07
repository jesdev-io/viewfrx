use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub const EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "flac", "ogg", "oga", "aif", "aiff", "m4a", "mp4", "aac",
];

pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| EXTENSIONS.contains(&s.to_ascii_lowercase().as_str()))
}

/// Discovery is extension-filtered; decoding is validated separately by inspect/play.
/// Each result is delivered immediately. Directory traversal does not follow symlinks.
pub fn discover(
    root: &Path,
    recursive: bool,
    emit: impl FnMut(Result<PathBuf>) -> Result<()>,
) -> Result<()> {
    discover_ordered(root, recursive, false, emit)
}

/// Optional per-directory alphabetical order. Sorting happens before emitting
/// that directory, so call off the UI thread for large/slow folders.
pub fn discover_ordered(
    root: &Path,
    recursive: bool,
    sorted: bool,
    mut emit: impl FnMut(Result<PathBuf>) -> Result<()>,
) -> Result<()> {
    if !root.exists() {
        bail!("path does not exist: {}", root.display());
    }
    if root.is_file() {
        if !is_audio_path(root) {
            bail!("unsupported audio extension: {}", root.display());
        }
        return emit(Ok(root.to_path_buf()));
    }
    let depth = if recursive { usize::MAX } else { 1 };
    let walker = WalkDir::new(root).max_depth(depth).follow_links(false);
    let walker = if sorted {
        walker.sort_by_file_name()
    } else {
        walker
    };
    for entry in walker {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_audio_path(entry.path()) => {
                emit(Ok(entry.into_path()))?
            }
            Ok(_) => {}
            Err(error) => emit(Err(error.into()))?,
        }
    }
    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub roots: Vec<PathBuf>,
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("invalid catalog")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn import(&mut self, paths: &[PathBuf]) -> Result<()> {
        // Validate the whole batch before mutating the catalog.
        let mut roots = Vec::new();
        for path in paths {
            let path = path
                .canonicalize()
                .with_context(|| format!("import {}", path.display()))?;
            if !path.is_dir() && !is_audio_path(&path) {
                bail!("unsupported audio extension: {}", path.display());
            }
            roots.push(path);
        }
        let mut seen: HashSet<_> = self.roots.iter().cloned().collect();
        self.roots
            .extend(roots.into_iter().filter(|p| seen.insert(p.clone())));
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        atomic_json(path, self)
    }
}

pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temp, value)?;
    temp.flush()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_filters_and_recurses() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("nested"))?;
        for name in ["A.WAV", "notes.txt", "nested/b.mp3"] {
            fs::write(dir.path().join(name), b"")?;
        }
        let mut paths = Vec::new();
        discover(dir.path(), false, |p| {
            paths.push(p?);
            Ok(())
        })?;
        assert_eq!(paths.len(), 1);
        paths.clear();
        discover(dir.path(), true, |p| {
            paths.push(p?);
            Ok(())
        })?;
        assert_eq!(paths.len(), 2);
        Ok(())
    }
    #[test]
    fn catalog_import_is_deduplicated_and_transactional() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut catalog = Catalog::default();
        catalog.import(&[dir.path().into(), dir.path().into()])?;
        assert_eq!(catalog.roots.len(), 1);
        assert!(catalog.import(&[dir.path().join("missing")]).is_err());
        assert_eq!(catalog.roots.len(), 1);
        let file = dir.path().join("catalog.json");
        catalog.save(&file)?;
        assert_eq!(Catalog::load(&file)?.roots, catalog.roots);
        Ok(())
    }
}
