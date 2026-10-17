//! A LaTeX project on disk: which file is the main one, what the build
//! depends on, and how to read file names out of a log.
//!
//! All functions here are blocking filesystem work; call them from
//! `spawn_blocking`.

use blake3::Hasher;
use latex_ai_core::security::is_denied;
use regex::Regex;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

/// Bump when the worker protocol or the way inputs are fed to the engine
/// changes, so old cached builds are not mistaken for current ones.
const FINGERPRINT_VERSION: &str = "m10-1";
/// The engine and bundle the PDFs come from. Bump with the `tectonic` pin in
/// the workspace `Cargo.toml` (see the warning there about blanket updates).
const ENGINE_VERSION: &str = "tectonic-0.16.9";

const MAX_PROJECT_FILES: usize = 20_000;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MainError {
    #[error("main file `{0}` does not exist")]
    NotFound(String),
    /// Several files look like roots and nothing says which: the caller
    /// should ask the user.
    #[error("cannot tell which file is the main one; candidates: {}", .0.join(", "))]
    Ambiguous(Vec<String>),
    #[error("no file with \\documentclass was found; set the main file")]
    NoRoot,
    #[error("reading the project: {0}")]
    Io(#[from] io::Error),
}

/// Project-relative paths of every regular file the build could read:
/// dot-directories, `node_modules`, `target`, symlinks and secrets
/// (`is_denied`) are left out. Sorted, so the result is deterministic.
///
/// # Errors
/// IO errors from the walk.
pub fn project_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            // `file_type` does not follow symlinks: a link is never walked.
            let kind = entry.file_type()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            if kind.is_dir() {
                if !name.starts_with('.') && name != "node_modules" && name != "target" {
                    dirs.push(path);
                }
            } else if kind.is_file() {
                let Ok(rel) = path.strip_prefix(root) else {
                    continue;
                };
                if !name.starts_with('.') && !is_denied(rel) {
                    files.push(rel.to_owned());
                    if files.len() > MAX_PROJECT_FILES {
                        return Err(io::Error::other(format!(
                            "project has more than {MAX_PROJECT_FILES} files"
                        )));
                    }
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Files that exist but must not be readable by the engine, as paths relative
/// to `root`. Unlike [`project_files`] this looks inside dot-directories.
///
/// # Errors
/// IO errors from the walk.
pub fn denied_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            visited += 1;
            if visited > MAX_PROJECT_FILES * 4 {
                return Ok(found);
            }
            if kind.is_dir() {
                if name != "node_modules" && name != "target" {
                    dirs.push(path);
                }
            } else if kind.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    if is_denied(rel) {
                        found.push(rel.to_owned());
                    }
                }
            }
        }
    }
    Ok(found)
}

/// Identity of a build: a hash of everything it can depend on. Equal
/// fingerprints mean the same PDF, so the second compile is free.
///
/// Every project file counts (not just `.tex`), because images, `.bib` and
/// `.sty` files all change the output.
///
/// # Errors
/// IO errors reading the project.
pub fn fingerprint(root: &Path, main: &str, want_synctex: bool) -> io::Result<String> {
    let mut hasher = Hasher::new();
    hasher.update(FINGERPRINT_VERSION.as_bytes());
    hasher.update(ENGINE_VERSION.as_bytes());
    hasher.update(&[u8::from(want_synctex)]);
    hasher.update(&(main.len() as u64).to_le_bytes());
    hasher.update(main.as_bytes());

    for rel in project_files(root)? {
        let name = rel.to_string_lossy();
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        let file = std::fs::File::open(root.join(&rel))?;
        let mut inner = Hasher::new();
        inner.update_reader(file)?;
        hasher.update(inner.finalize().as_bytes());
    }
    Ok(hasher.finalize().to_hex()[..32].to_owned())
}

static DOCUMENTCLASS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?m)^[^%\n]*\\documentclass\b").ok());
static TEX_ROOT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?im)^\s*%\s*!TEX\s+root\s*=\s*(\S.*?)\s*$").ok());

fn has_documentclass(text: &str) -> bool {
    DOCUMENTCLASS.as_ref().is_some_and(|re| re.is_match(text))
}

/// Normalise `rel` lexically and refuse anything that leaves `root`.
fn normalise_inside(rel: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Decide which file to compile.
///
/// In order: an explicit `override_main`; a `% !TEX root = …` magic comment
/// in the active file; the active file itself if it has `\documentclass`;
/// the one file in the project that does (or the one root that includes the
/// active file). When several roots are plausible the answer is
/// [`MainError::Ambiguous`] rather than a guess.
///
/// # Errors
/// See [`MainError`].
pub fn detect_main(
    root: &Path,
    active: &str,
    override_main: Option<&str>,
) -> Result<String, MainError> {
    let exists = |rel: &str| root.join(rel).is_file();
    let path_str = |p: &Path| p.to_string_lossy().into_owned();

    if let Some(m) = override_main {
        let rel = normalise_inside(Path::new(m)).ok_or_else(|| MainError::NotFound(m.to_owned()))?;
        return if exists(&path_str(&rel)) {
            Ok(path_str(&rel))
        } else {
            Err(MainError::NotFound(m.to_owned()))
        };
    }

    let active_text = std::fs::read_to_string(root.join(active)).unwrap_or_default();

    let head = active_text.lines().take(20).collect::<Vec<_>>().join("\n");
    if let Some(caps) = TEX_ROOT.as_ref().and_then(|re| re.captures(&head)) {
        let dir = Path::new(active).parent().unwrap_or(Path::new(""));
        let declared = dir.join(caps[1].trim());
        let rel = normalise_inside(&declared)
            .ok_or_else(|| MainError::NotFound(caps[1].trim().to_owned()))?;
        return if exists(&path_str(&rel)) {
            Ok(path_str(&rel))
        } else {
            Err(MainError::NotFound(path_str(&rel)))
        };
    }

    if has_documentclass(&active_text) {
        return Ok(active.to_owned());
    }

    let mut roots = Vec::new();
    for rel in project_files(root)? {
        if rel.extension().and_then(|e| e.to_str()) != Some("tex") {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&rel)).unwrap_or_default();
        if has_documentclass(&text) {
            roots.push((path_str(&rel), text));
        }
    }

    match roots.as_slice() {
        [] => Err(MainError::NoRoot),
        [(only, _)] => Ok(only.clone()),
        many => {
            // Prefer the root that actually pulls the active file in.
            let stem = Path::new(active)
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            let including: Vec<&String> = many
                .iter()
                .filter(|(_, text)| includes(text, &stem))
                .map(|(name, _)| name)
                .collect();
            match including.as_slice() {
                [one] => Ok((*one).clone()),
                _ => Err(MainError::Ambiguous(many.iter().map(|(n, _)| n.clone()).collect())),
            }
        }
    }
}

/// Whether `text` has an `\input`/`\include`/… line naming `stem`.
fn includes(text: &str, stem: &str) -> bool {
    text.lines().any(|line| {
        let code = line.split('%').next().unwrap_or("");
        ["\\input", "\\include", "\\subfile", "\\import"]
            .iter()
            .any(|cmd| code.contains(cmd))
            && code.contains(stem)
    })
}

/// Turn a file name as printed in a TeX log (relative to the main file's
/// directory, often without `.tex`) into a project-relative path, or `None`
/// when it is not a project file.
#[must_use]
pub fn resolve_log_name(root: &Path, main_dir: &Path, token: &str) -> Option<String> {
    let token = token.trim_matches('"');
    if Path::new(token).is_absolute() {
        return None;
    }
    let main_rel = main_dir.strip_prefix(root).ok()?;
    [token.to_owned(), format!("{token}.tex")]
        .into_iter()
        .filter_map(|candidate| normalise_inside(&main_rel.join(candidate)))
        .find(|rel| root.join(rel).is_file())
        .map(|rel| rel.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    const ROOT_TEX: &str = "\\documentclass{article}\n\\begin{document}\n\\input{chapters/a}\n\\end{document}\n";

    #[test]
    fn the_active_file_is_main_when_it_has_documentclass() {
        let p = project(&[("main.tex", ROOT_TEX), ("other.tex", ROOT_TEX)]);
        assert_eq!(detect_main(p.path(), "other.tex", None).unwrap(), "other.tex");
    }

    #[test]
    fn a_chapter_resolves_to_the_only_root_in_the_project() {
        let p = project(&[("main.tex", ROOT_TEX), ("chapters/a.tex", "text\n")]);
        assert_eq!(detect_main(p.path(), "chapters/a.tex", None).unwrap(), "main.tex");
    }

    #[test]
    fn a_commented_documentclass_does_not_make_a_root() {
        let p = project(&[
            ("main.tex", ROOT_TEX),
            ("notes.tex", "% \\documentclass{article}\nnotes\n"),
        ]);
        assert_eq!(detect_main(p.path(), "notes.tex", None).unwrap(), "main.tex");
    }

    #[test]
    fn magic_comment_wins_and_is_relative_to_the_active_file() {
        let p = project(&[
            ("thesis/main.tex", ROOT_TEX),
            ("other.tex", ROOT_TEX),
            ("thesis/ch/one.tex", "% !TEX root = ../main.tex\ntext\n"),
        ]);
        assert_eq!(
            detect_main(p.path(), "thesis/ch/one.tex", None).unwrap(),
            "thesis/main.tex"
        );
    }

    #[test]
    fn a_magic_comment_cannot_point_outside_the_project() {
        let p = project(&[("ch.tex", "% !TEX root = ../../etc/passwd\n")]);
        assert!(matches!(
            detect_main(p.path(), "ch.tex", None),
            Err(MainError::NotFound(_))
        ));
    }

    #[test]
    fn explicit_override_beats_everything_and_must_exist() {
        let p = project(&[("main.tex", ROOT_TEX), ("alt.tex", ROOT_TEX)]);
        assert_eq!(detect_main(p.path(), "main.tex", Some("alt.tex")).unwrap(), "alt.tex");
        assert!(matches!(
            detect_main(p.path(), "main.tex", Some("missing.tex")),
            Err(MainError::NotFound(_))
        ));
        assert!(matches!(
            detect_main(p.path(), "main.tex", Some("../escape.tex")),
            Err(MainError::NotFound(_))
        ));
    }

    #[test]
    fn several_roots_are_ambiguous_unless_one_includes_the_active_file() {
        let p = project(&[
            ("a.tex", ROOT_TEX),
            ("b.tex", "\\documentclass{book}\n\\begin{document}\n\\include{chapters/x}\n\\end{document}\n"),
            ("chapters/x.tex", "x\n"),
            ("chapters/y.tex", "y\n"),
        ]);
        // b.tex includes chapters/x, so that is the answer for x...
        assert_eq!(detect_main(p.path(), "chapters/x.tex", None).unwrap(), "b.tex");
        // ...but nothing includes y, so the user must say.
        match detect_main(p.path(), "chapters/y.tex", None) {
            Err(MainError::Ambiguous(c)) => assert_eq!(c, ["a.tex", "b.tex"]),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn a_project_with_no_root_says_so() {
        let p = project(&[("notes.tex", "just text\n")]);
        assert!(matches!(detect_main(p.path(), "notes.tex", None), Err(MainError::NoRoot)));
    }

    #[test]
    fn project_files_skip_secrets_dotdirs_and_symlinks() {
        let p = project(&[
            ("main.tex", "x"),
            ("img/fig.png", "png"),
            (".env", "SECRET=1"),
            (".git/config", "c"),
            ("node_modules/x.js", "j"),
            ("keys/server.pem", "pem"),
        ]);
        std::os::unix::fs::symlink(p.path(), p.path().join("loop")).unwrap();
        let files: Vec<_> = project_files(p.path())
            .unwrap()
            .iter()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, ["img/fig.png", "main.tex"]);
    }

    #[test]
    fn denied_files_looks_inside_dot_directories() {
        let p = project(&[("main.tex", "x"), (".env", "S"), (".ssh/id_rsa", "k"), ("sub/server.pem", "p")]);
        let mut denied: Vec<_> = denied_files(p.path())
            .unwrap()
            .iter()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        denied.sort();
        assert_eq!(denied, [".env", ".ssh/id_rsa", "sub/server.pem"]);
    }

    #[test]
    fn fingerprint_changes_with_any_input_and_only_then() {
        let p = project(&[("main.tex", ROOT_TEX), ("chapters/a.tex", "one\n"), ("fig.png", "v1")]);
        let base = fingerprint(p.path(), "main.tex", true).unwrap();
        assert_eq!(base, fingerprint(p.path(), "main.tex", true).unwrap(), "must be stable");
        assert_eq!(base.len(), 32);

        // An included file, an image, and a new file each bust the cache.
        std::fs::write(p.path().join("chapters/a.tex"), "two\n").unwrap();
        let after_include = fingerprint(p.path(), "main.tex", true).unwrap();
        assert_ne!(base, after_include);
        std::fs::write(p.path().join("fig.png"), "v2").unwrap();
        let after_image = fingerprint(p.path(), "main.tex", true).unwrap();
        assert_ne!(after_include, after_image);
        std::fs::write(p.path().join("refs.bib"), "@book{}").unwrap();
        assert_ne!(after_image, fingerprint(p.path(), "main.tex", true).unwrap());

        // The flags and the entry point are part of the identity too.
        assert_ne!(base, fingerprint(p.path(), "main.tex", false).unwrap());
        assert_ne!(base, fingerprint(p.path(), "chapters/a.tex", true).unwrap());

        // A secret appearing or changing does not.
        let stable = fingerprint(p.path(), "main.tex", true).unwrap();
        std::fs::write(p.path().join(".env"), "S=1").unwrap();
        assert_eq!(stable, fingerprint(p.path(), "main.tex", true).unwrap());
    }

    #[test]
    fn log_names_resolve_relative_to_the_main_files_directory() {
        let p = project(&[("doc/main.tex", ROOT_TEX), ("doc/chapters/a.tex", "a"), ("doc/refs.bib", "b")]);
        let root = p.path();
        let main_dir = root.join("doc");
        assert_eq!(resolve_log_name(root, &main_dir, "chapters/a").as_deref(), Some("doc/chapters/a.tex"));
        assert_eq!(resolve_log_name(root, &main_dir, "./chapters/a.tex").as_deref(), Some("doc/chapters/a.tex"));
        assert_eq!(resolve_log_name(root, &main_dir, "refs.bib").as_deref(), Some("doc/refs.bib"));
        // Not project files: bundle packages, and things that do not exist.
        assert_eq!(resolve_log_name(root, &main_dir, "article.cls"), None);
        assert_eq!(resolve_log_name(root, &main_dir, "../../etc/passwd"), None);
    }
}
