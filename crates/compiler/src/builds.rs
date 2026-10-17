//! The build store: finished compiles kept outside the project.
//!
//! `<home>/projects/<id>/builds/<build id>/` holds `output.pdf`,
//! `output.synctex.gz`, `output.log` and `manifest.json`. The build id is the
//! project fingerprint, so an unchanged project maps to an existing build
//! and the PDF URL (which carries the id) never changes meaning — an
//! `immutable` cache header is safe, and a page image can never be stale.
//!
//! A build is published by renaming a fully written directory into place, so
//! a reader sees all of a build or none of it.

use crate::diagnostics::Diagnostic;
use crate::worker::{LOG, PDF, SYNCTEX};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Everything we remember about one compile. Written once, never changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub build_id: String,
    /// Main file, project-relative.
    pub main: String,
    pub success: bool,
    pub has_pdf: bool,
    pub has_synctex: bool,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub diagnostics: Vec<Diagnostic>,
}

/// A build on disk.
#[derive(Debug, Clone)]
pub struct Build {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Build {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.manifest.build_id
    }

    #[must_use]
    pub fn pdf_path(&self) -> PathBuf {
        self.dir.join(PDF)
    }

    #[must_use]
    pub fn synctex_path(&self) -> PathBuf {
        self.dir.join(SYNCTEX)
    }

    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        self.dir.join(LOG)
    }
}

/// Ids are hex fingerprints; anything else is refused before it can become a
/// path (the API takes the id from the URL).
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

const MANIFEST: &str = "manifest.json";
const TMP_PREFIX: &str = ".tmp-";

#[derive(Debug, Clone)]
pub struct Builds {
    dir: PathBuf,
    keep: usize,
}

impl Builds {
    #[must_use]
    pub fn new(dir: PathBuf, keep: usize) -> Self {
        Self {
            dir,
            keep: keep.max(1),
        }
    }

    /// A fresh private directory for a worker to write into.
    ///
    /// # Errors
    /// IO errors creating it.
    pub async fn new_scratch_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self.dir.join(format!(
            "{TMP_PREFIX}{}",
            latex_ai_core::fsutil::unique_id()
        ));
        tokio::fs::create_dir_all(&dir).await?;
        Ok(dir)
    }

    /// The build with this id, if it exists and its manifest is readable.
    pub async fn get(&self, id: &str) -> Option<Build> {
        if !is_valid_id(id) {
            return None;
        }
        let dir = self.dir.join(id);
        let text = tokio::fs::read_to_string(dir.join(MANIFEST)).await.ok()?;
        match serde_json::from_str::<Manifest>(&text) {
            Ok(manifest) if manifest.build_id == id => Some(Build { dir, manifest }),
            Ok(_) | Err(_) => {
                tracing::warn!(build = id, "ignoring a build with an unreadable manifest");
                None
            }
        }
    }

    /// The newest build of `main` that produced a PDF.
    pub async fn latest_with_pdf(&self, main: &str) -> Option<Build> {
        self.all()
            .await
            .into_iter()
            .filter(|b| b.manifest.main == main && b.manifest.has_pdf)
            .max_by_key(|b| b.manifest.created)
    }

    /// Move a fully written scratch directory into place as build
    /// `manifest.build_id`. If that build already exists (a concurrent
    /// compile of the same inputs finished first) the existing one wins and
    /// the scratch directory is discarded.
    ///
    /// # Errors
    /// IO errors writing the manifest or renaming.
    pub async fn publish(&self, scratch: &Path, manifest: Manifest) -> std::io::Result<Build> {
        let json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        tokio::fs::write(scratch.join(MANIFEST), json).await?;

        let target = self.dir.join(&manifest.build_id);
        match tokio::fs::rename(scratch, &target).await {
            Ok(()) => {}
            Err(_) if tokio::fs::metadata(&target).await.is_ok() => {
                let _ = tokio::fs::remove_dir_all(scratch).await;
            }
            Err(e) => return Err(e),
        }
        self.prune().await;
        Ok(Build {
            dir: target,
            manifest,
        })
    }

    /// Delete a scratch directory that will not be published.
    pub async fn discard(&self, scratch: &Path) {
        if let Err(e) = tokio::fs::remove_dir_all(scratch).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, "could not remove a scratch build directory");
            }
        }
    }

    async fn all(&self) -> Vec<Build> {
        let mut out = Vec::new();
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else {
            return out;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(id) = entry.file_name().to_str() {
                if let Some(build) = self.get(id).await {
                    out.push(build);
                }
            }
        }
        out
    }

    /// Keep the newest `keep` builds; remove the rest and any scratch
    /// directory a crashed compile left behind.
    async fn prune(&self) {
        let mut builds = self.all().await;
        builds.sort_by_key(|b| std::cmp::Reverse(b.manifest.created));
        for old in builds.into_iter().skip(self.keep) {
            self.discard(&old.dir).await;
        }

        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let is_stale_scratch = entry.file_name().to_string_lossy().starts_with(TMP_PREFIX)
                && entry
                    .metadata()
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > std::time::Duration::from_secs(3600));
            if is_stale_scratch {
                self.discard(&entry.path()).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, main: &str, created: u64, has_pdf: bool) -> Manifest {
        Manifest {
            build_id: id.to_owned(),
            main: main.to_owned(),
            success: has_pdf,
            has_pdf,
            has_synctex: false,
            created,
            diagnostics: Vec::new(),
        }
    }

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccc";

    async fn publish(builds: &Builds, m: Manifest) -> Build {
        let scratch = builds.new_scratch_dir().await.unwrap();
        tokio::fs::write(scratch.join(PDF), b"%PDF-fake")
            .await
            .unwrap();
        builds.publish(&scratch, m).await.unwrap()
    }

    #[test]
    fn only_32_hex_characters_are_valid_ids() {
        assert!(is_valid_id(A));
        for bad in [
            "",
            "abc",
            "../etc/passwd",
            &"g".repeat(32),
            &format!("{A}0"),
            "a/../aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn a_published_build_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let builds = Builds::new(dir.path().to_owned(), 5);
        let published = publish(&builds, manifest(A, "main.tex", 10, true)).await;

        let got = builds.get(A).await.unwrap();
        assert_eq!(got.id(), A);
        assert_eq!(got.manifest.main, "main.tex");
        assert_eq!(std::fs::read(got.pdf_path()).unwrap(), b"%PDF-fake");
        assert_eq!(published.dir, got.dir);
        assert!(builds.get(B).await.is_none());
        assert!(builds.get("../x").await.is_none());
    }

    #[tokio::test]
    async fn publishing_over_an_existing_build_keeps_the_first_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let builds = Builds::new(dir.path().to_owned(), 5);
        publish(&builds, manifest(A, "main.tex", 10, true)).await;
        let second = publish(&builds, manifest(A, "main.tex", 99, true)).await;

        // The loser's manifest is not what is on disk.
        assert_eq!(builds.get(A).await.unwrap().manifest.created, 10);
        assert_eq!(second.dir, dir.path().join(A));
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(TMP_PREFIX))
            .collect();
        assert!(leftovers.is_empty(), "scratch dir leaked");
    }

    #[tokio::test]
    async fn old_builds_are_pruned_newest_kept() {
        let dir = tempfile::tempdir().unwrap();
        let builds = Builds::new(dir.path().to_owned(), 2);
        publish(&builds, manifest(A, "main.tex", 1, true)).await;
        publish(&builds, manifest(B, "main.tex", 2, true)).await;
        publish(&builds, manifest(C, "main.tex", 3, true)).await;

        assert!(builds.get(A).await.is_none(), "oldest should be gone");
        assert!(builds.get(B).await.is_some());
        assert!(builds.get(C).await.is_some());
    }

    #[tokio::test]
    async fn latest_with_pdf_skips_failures_and_other_mains() {
        let dir = tempfile::tempdir().unwrap();
        let builds = Builds::new(dir.path().to_owned(), 9);
        publish(&builds, manifest(A, "main.tex", 1, true)).await;
        publish(&builds, manifest(B, "main.tex", 2, false)).await; // failed compile, newer
        publish(&builds, manifest(C, "other.tex", 3, true)).await;

        assert_eq!(builds.latest_with_pdf("main.tex").await.unwrap().id(), A);
        assert!(builds.latest_with_pdf("none.tex").await.is_none());
    }

    #[tokio::test]
    async fn a_corrupt_manifest_is_not_a_build() {
        let dir = tempfile::tempdir().unwrap();
        let builds = Builds::new(dir.path().to_owned(), 5);
        std::fs::create_dir_all(dir.path().join(A)).unwrap();
        std::fs::write(dir.path().join(A).join(MANIFEST), "not json").unwrap();
        assert!(builds.get(A).await.is_none());
        // A manifest that claims a different id is not trusted either.
        std::fs::write(
            dir.path().join(A).join(MANIFEST),
            serde_json::to_string(&manifest(B, "m.tex", 1, true)).unwrap(),
        )
        .unwrap();
        assert!(builds.get(A).await.is_none());
    }
}
