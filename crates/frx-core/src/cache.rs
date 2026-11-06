//! Shared source fingerprint and conservative cache eviction policy.
use anyhow::Result;
use std::{fs, path::Path, time::UNIX_EPOCH};

pub(crate) fn key(path: &Path, parameters: &[u8]) -> Result<String> {
    let canonical = path.canonicalize()?;
    let meta = canonical.metadata()?;
    let modified = meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
    let mut hash = blake3::Hasher::new();
    hash.update(canonical.as_os_str().as_encoded_bytes());
    hash.update(&meta.len().to_le_bytes());
    hash.update(&modified.to_le_bytes());
    hash.update(parameters);
    Ok(hash.finalize().to_hex().to_string())
}

pub(crate) fn prune(directory: &Path, prefix: &str, budget: u64) -> Result<()> {
    let mut entries = Vec::new();
    let mut total = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let owned = name
            .to_str()
            .and_then(|s| s.strip_prefix(prefix))
            .and_then(|s| s.strip_suffix(".json"))
            .is_some_and(|key| {
                key.len() == blake3::OUT_LEN * 2 && key.bytes().all(|b| b.is_ascii_hexdigit())
            });
        if owned && entry.file_type()?.is_file() {
            // Another analysis process may evict between read_dir and metadata.
            let Ok(meta) = entry.metadata() else { continue };
            total += meta.len();
            entries.push((meta.modified()?, meta.len(), entry.path()));
        }
    }
    entries.sort_by_key(|entry| entry.0);
    for (_, bytes, path) in entries {
        if total <= budget {
            break;
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        total -= bytes;
    }
    Ok(())
}
