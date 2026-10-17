//! Crash-safe file replacement (rust.md I.13, idea.md §7 contract 1).

use std::path::Path;
use tokio::io::AsyncWriteExt;

/// Replace `path` with `content` so a reader sees either the old or the new
/// bytes, never a mix.
///
/// The temp file lives beside the target (same filesystem, so `rename` is
/// atomic) and carries a random id: `a.tex` and `a.bib` used to share
/// `a.tmp`, and two processes could clobber each other's temp file. It is
/// fsynced before the rename so a power cut cannot leave an empty target.
///
/// # Errors
/// IO errors from creating, writing, syncing or renaming. The temp file is
/// removed on every failure path.
pub async fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    replace(path, content, false).await
}

/// [`atomic_write`] for secrets: the temp file is created `0600` (on Unix),
/// so the key is never readable by other users, not even for the instant
/// before a `chmod`. An existing target's mode is deliberately not copied.
///
/// # Errors
/// As [`atomic_write`].
pub async fn atomic_write_private(path: &Path, content: &[u8]) -> std::io::Result<()> {
    replace(path, content, true).await
}

async fn replace(path: &Path, content: &[u8], private: bool) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| std::io::Error::other("path has no file name"))?;
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", unique_id()));

    let result = write_synced(&tmp, content, private).await;
    let result = match result {
        Ok(()) if private => Ok(()),
        Ok(()) => preserve_permissions(path, &tmp).await,
        Err(e) => Err(e),
    };
    let result = match result {
        Ok(()) => tokio::fs::rename(&tmp, path).await,
        Err(e) => Err(e),
    };

    if result.is_err() {
        if let Err(e) = tokio::fs::remove_file(&tmp).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, tmp = %tmp.display(), "could not remove temp file");
            }
        }
    }
    result
}

async fn write_synced(tmp: &Path, content: &[u8], private: bool) -> std::io::Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(tmp).await?;
    file.write_all(content).await?;
    file.sync_all().await
}

/// Keep the target's mode (a script's exec bit, a read-only file's mode)
/// instead of silently resetting it to the temp file's default.
async fn preserve_permissions(target: &Path, tmp: &Path) -> std::io::Result<()> {
    match tokio::fs::metadata(target).await {
        Ok(meta) => tokio::fs::set_permissions(tmp, meta.permissions()).await,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// 128 random bits as hex, from the blake3 XOF seeded with time + a counter +
/// the address of a stack local; unique per call within and across processes,
/// which is all a temp-file name needs (not a secret).
#[must_use]
pub fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let local = 0u8;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&nanos.to_le_bytes());
    hasher.update(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&(std::ptr::from_ref(&local) as usize).to_le_bytes());
    let mut out = [0u8; 16];
    hasher.finalize_xof().fill(&mut out);
    hex::encode(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaces_content_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.tex");
        std::fs::write(&target, "old").unwrap();

        atomic_write(&target, b"new").await.unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "temp file leaked: {leftovers:?}");
    }

    #[tokio::test]
    async fn sibling_files_with_the_same_stem_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let tex = dir.path().join("a.tex");
        let bib = dir.path().join("a.bib");

        let (r1, r2) = tokio::join!(atomic_write(&tex, b"tex"), atomic_write(&bib, b"bib"));
        r1.unwrap();
        r2.unwrap();

        assert_eq!(std::fs::read_to_string(tex).unwrap(), "tex");
        assert_eq!(std::fs::read_to_string(bib).unwrap(), "bib");
    }

    #[tokio::test]
    async fn failure_removes_the_temp_file_and_keeps_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("keep.tex");
        std::fs::write(&target, "old").unwrap();
        // Renaming a file over a directory fails on every platform.
        let blocker = dir.path().join("blocked");
        std::fs::create_dir(&blocker).unwrap();

        assert!(atomic_write(&blocker, b"x").await.is_err());

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| !n.to_ascii_lowercase().ends_with(".tmp")),
            "temp file leaked: {names:?}"
        );
    }

    #[tokio::test]
    async fn private_writes_are_owner_only_even_over_a_wider_target() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secrets.toml");
        std::fs::write(&target, "old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

        atomic_write_private(&target, b"new").await.unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("script.tex");
        std::fs::write(&target, "old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();

        atomic_write(&target, b"new").await.unwrap();

        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }
}
