//! Where preflight profiles come from: the ones that ship, and the ones the
//! user keeps in `<home>/profiles/*.toml` (a file there replaces a shipped
//! profile of the same id). A profile can also be named by a path.

use preflight::Profile;
use std::path::{Path, PathBuf};

/// Every profile available, shipped first, user's last. A file that does not
/// parse is skipped with a warning rather than hiding the others.
#[must_use]
pub fn all(home: Option<&Path>) -> Vec<Profile> {
    let mut profiles = preflight::built_in();
    let Some(dir) = home.map(|h| h.join("profiles")) else {
        return profiles;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return profiles;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .collect();
    files.sort();
    for path in files {
        match load(&path) {
            Ok(profile) => {
                profiles.retain(|p| p.id != profile.id);
                profiles.push(profile);
            }
            Err(e) => tracing::warn!(file = %path.display(), error = %e, "skipping a profile"),
        }
    }
    profiles
}

fn load(path: &Path) -> Result<Profile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Profile::parse(&text).map_err(|e| e.to_string())
}

/// The profile named `spec`: an id, or the path to a profile file.
///
/// # Errors
/// Says what is wrong and which ids exist.
pub fn find(spec: &str, home: Option<&Path>) -> Result<Profile, String> {
    let path = Path::new(spec);
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        || path.is_file()
    {
        return load(path).map_err(|e| format!("{spec}: {e}"));
    }
    let profiles = all(home);
    let known = profiles
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>()
        .join(", ");
    profiles
        .into_iter()
        .find(|p| p.id == spec)
        .ok_or_else(|| format!("no profile `{spec}`; available: {known}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINE: &str = "id = \"arxiv\"\nname = \"My arXiv\"\n[[rule]]\ncheck = \"alt_text\"\n";

    #[test]
    fn a_user_profile_replaces_a_shipped_one_with_the_same_id_and_bad_files_are_skipped() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("profiles")).unwrap();
        std::fs::write(home.path().join("profiles/arxiv.toml"), MINE).unwrap();
        std::fs::write(home.path().join("profiles/broken.toml"), "id = ").unwrap();
        std::fs::write(home.path().join("profiles/notes.txt"), "ignored").unwrap();

        let found = all(Some(home.path()));
        let arxiv: Vec<_> = found.iter().filter(|p| p.id == "arxiv").collect();
        assert_eq!(arxiv.len(), 1);
        assert_eq!(arxiv[0].name, "My arXiv");
        assert_eq!(found.len(), preflight::built_in().len());
    }

    #[test]
    fn a_profile_is_found_by_id_or_by_path_and_a_miss_lists_the_ids() {
        assert_eq!(find("double-blind", None).unwrap().id, "double-blind");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("venue.toml");
        std::fs::write(&file, "id = \"venue\"\nname = \"V\"\n").unwrap();
        assert_eq!(find(file.to_str().unwrap(), None).unwrap().id, "venue");
        let err = find("nope", None).unwrap_err();
        assert!(
            err.contains("no profile `nope`") && err.contains("arxiv"),
            "{err}"
        );
    }
}
