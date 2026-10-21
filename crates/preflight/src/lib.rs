//! Submission checks for a LaTeX project.
//!
//! A [`Profile`] (data, see [`profile`]) lists the checks a venue or a purpose
//! wants; [`run`] applies them to a [`Project`] read from disk and the
//! [`Facts`] a build produced (page count, fonts, PDF metadata), and returns a
//! [`Report`] of findings, each with a file and line when it has one.
//!
//! The checks read source text with a scanner, so they see what is written,
//! not what a macro would expand to. A check that cannot know says so rather
//! than passing: a missing fact is reported as an `Info`, not as success.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod checks;
pub mod profile;
mod tex;

pub use profile::{Check, Profile, ProfileError, Rule, Severity};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// The profiles that ship with Renkeu.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("arxiv", include_str!("../profiles/arxiv.toml")),
    (
        "double-blind",
        include_str!("../profiles/double-blind.toml"),
    ),
    (
        "accessibility",
        include_str!("../profiles/accessibility.toml"),
    ),
];

/// The shipped profiles, parsed.
///
/// # Panics
/// Never in a released build: a test parses every one.
#[must_use]
pub fn built_in() -> Vec<Profile> {
    BUILT_IN
        .iter()
        .filter_map(|(_, text)| Profile::parse(text).ok())
        .collect()
}

/// Largest file whose text is read, and most files listed.
const MAX_TEXT_BYTES: u64 = 2_000_000;
const MAX_FILES: usize = 5_000;
/// Extensions whose content the checks look at.
const TEXT_EXTENSIONS: &[&str] = &[
    "tex", "sty", "cls", "bib", "bbl", "bst", "ind", "ltx", "def",
];

#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    pub size: u64,
    /// The text, for the file types the checks read.
    pub text: Option<String>,
}

/// The files of a project, by project-relative path with `/` separators.
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub main: String,
    pub files: BTreeMap<String, FileInfo>,
}

impl Project {
    /// Read the project in `root`. Symbolic links are not followed, and `.git`
    /// is listed as one hidden entry rather than walked.
    ///
    /// # Errors
    /// The folder cannot be read.
    pub fn read(root: &Path, main: &str) -> std::io::Result<Self> {
        let mut files = BTreeMap::new();
        walk(root, root, &mut files)?;
        Ok(Self {
            main: main.to_owned(),
            files,
        })
    }

    fn text_files(&self) -> impl Iterator<Item = (&String, &String)> {
        self.files
            .iter()
            .filter_map(|(path, info)| info.text.as_ref().map(|t| (path, t)))
    }

    fn is_tex(path: &str) -> bool {
        path.rsplit('.')
            .next()
            .is_some_and(|e| e.eq_ignore_ascii_case("tex"))
    }
}

fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, FileInfo>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = std::fs::symlink_metadata(entry.path())?;
        let rel = entry
            .path()
            .strip_prefix(root)
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        if files.len() >= MAX_FILES || meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if entry.file_name().to_string_lossy() == ".git" {
                files.insert(format!("{rel}/"), FileInfo::default());
            } else {
                walk(root, &entry.path(), files)?;
            }
        } else if meta.is_file() {
            let text_type = TEXT_EXTENSIONS.iter().any(|e| {
                rel.rsplit('.')
                    .next()
                    .is_some_and(|x| x.eq_ignore_ascii_case(e))
            });
            let text = (text_type && meta.len() <= MAX_TEXT_BYTES)
                .then(|| std::fs::read(entry.path()).ok())
                .flatten()
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            files.insert(
                rel,
                FileInfo {
                    size: meta.len(),
                    text,
                },
            );
        }
    }
    Ok(())
}

/// What a build knows that the source does not.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub pages: Option<u32>,
    /// `None` when the PDF could not be inspected.
    pub fonts_not_embedded: Option<Vec<String>>,
    /// The Author field of the PDF; `None` when it could not be read.
    pub pdf_author: Option<String>,
    /// The build produced a `.bbl` (BibTeX ran and wrote one).
    pub built_bbl: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The check that found it, as named in profiles.
    pub check: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub profile: String,
    pub name: String,
    pub source: Option<String>,
    pub findings: Vec<Finding>,
    /// No finding is an error.
    pub passed: bool,
}

/// Apply `profile` to `project`.
#[must_use]
pub fn run(profile: &Profile, project: &Project, facts: &Facts) -> Report {
    let mut findings = Vec::new();
    for rule in &profile.rules {
        for mut finding in checks::run(&rule.check, project, facts) {
            finding.severity = rule.severity.min(finding.severity);
            if let Some(custom) = &rule.message
                && !matches!(rule.check, Check::ForbidPattern { .. })
            {
                finding.message = format!("{custom} ({})", finding.message);
            }
            findings.push(finding);
        }
    }
    let passed = findings.iter().all(|f| f.severity != Severity::Error);
    Report {
        profile: profile.id.clone(),
        name: profile.name.clone(),
        source: profile.source.clone(),
        findings,
        passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_profile_parses_and_has_a_source_it_can_be_checked_against() {
        for (id, text) in BUILT_IN {
            let p = Profile::parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(&p.id, id);
            assert!(!p.rules.is_empty(), "{id}");
            assert!(!p.description.is_empty(), "{id} says nothing about itself");
        }
        assert_eq!(built_in().len(), BUILT_IN.len());
    }
}
