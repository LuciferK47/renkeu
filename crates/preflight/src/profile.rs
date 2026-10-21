//! Profiles: what to check before submitting, as plain data.
//!
//! The code knows *how* to check things; a profile says *which* checks a
//! venue or a purpose wants, with what numbers, and where that comes from.
//! Nothing here claims what any venue requires: the claims live in the
//! profile files, each with a `source` its author can point to, and anyone can
//! edit or add one.
//!
//! ```toml
//! id = "my-venue"
//! name = "My venue"
//! source = "https://example.org/author-instructions"
//!
//! [[rule]]
//! check = "page_limit"
//! pages = 8
//!
//! [[rule]]
//! check = "forbid_pattern"
//! pattern = '\\todo'
//! message = "Remove the todo notes."
//! severity = "warning"
//! ```

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A check the code knows how to run, with its settings.
#[derive(Debug, Clone)]
pub enum Check {
    /// The PDF has at most this many pages.
    PageLimit { pages: u32 },
    /// Every font in the PDF is embedded.
    FontsEmbedded,
    /// No `\input`, `\includegraphics` and so on reaches outside the project.
    ProjectPathsOnly,
    /// File names use only these characters (see `allowed`).
    PortableFileNames { allowed: Regex },
    /// A project that uses BibTeX includes the `.bbl` named after the main file.
    BblPresent,
    /// No hidden files or folders.
    NoHiddenFiles,
    /// Files nothing in the project refers to.
    UnusedFiles,
    /// The project is at most this large, in megabytes.
    MaxTotalSize { megabytes: f64 },
    /// Comment lines are left in the source.
    CommentsLeft,
    /// `\author` and friends carry no names, emails or affiliations.
    AnonymousAuthors,
    /// No acknowledgements, funding statement or `\thanks`.
    NoAcknowledgements,
    /// Prose that points back at the authors' own earlier work.
    NoSelfReference,
    /// The PDF's own metadata names no author.
    PdfMetadataClean,
    /// Every image has alt text.
    AltText,
    /// A pattern that must not appear in the source.
    ForbidPattern { pattern: Regex, message: String },
}

impl Check {
    /// The name used in profile files and in findings.
    #[must_use]
    pub fn id(&self) -> &'static str {
        match self {
            Self::PageLimit { .. } => "page_limit",
            Self::FontsEmbedded => "fonts_embedded",
            Self::ProjectPathsOnly => "project_paths_only",
            Self::PortableFileNames { .. } => "portable_file_names",
            Self::BblPresent => "bbl_present",
            Self::NoHiddenFiles => "no_hidden_files",
            Self::UnusedFiles => "unused_files",
            Self::MaxTotalSize { .. } => "max_total_size",
            Self::CommentsLeft => "comments_left",
            Self::AnonymousAuthors => "anonymous_authors",
            Self::NoAcknowledgements => "no_acknowledgements",
            Self::NoSelfReference => "no_self_reference",
            Self::PdfMetadataClean => "pdf_metadata_clean",
            Self::AltText => "alt_text",
            Self::ForbidPattern { .. } => "forbid_pattern",
        }
    }

    /// How serious a finding is when the rule does not say.
    fn default_severity(&self) -> Severity {
        match self {
            Self::UnusedFiles | Self::CommentsLeft | Self::AltText => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub check: Check,
    pub severity: Severity,
    /// Replaces the check's own wording, to explain what the venue asks.
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Where the rules come from, for whoever has to trust them.
    pub source: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    #[error("not a profile: {0}")]
    Syntax(String),
    #[error("rule {number}: {problem}")]
    Rule { number: usize, problem: String },
    #[error("a profile needs an `id` of letters, digits, `-` and `_`")]
    BadId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default, rename = "rule")]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    check: String,
    #[serde(default)]
    severity: Option<Severity>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    pages: Option<u32>,
    #[serde(default)]
    megabytes: Option<f64>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    allowed: Option<String>,
}

/// The characters arXiv accepts in file names, as a pattern for one name.
pub const DEFAULT_FILE_NAME: &str = r"^[A-Za-z0-9_+\-.,=/]+$";

impl Profile {
    /// Read a profile from TOML.
    ///
    /// # Errors
    /// [`ProfileError`] naming the rule and what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, ProfileError> {
        let raw: RawProfile =
            toml::from_str(text).map_err(|e| ProfileError::Syntax(e.message().to_owned()))?;
        if raw.id.is_empty()
            || !raw
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ProfileError::BadId);
        }
        let rules = raw
            .rules
            .into_iter()
            .enumerate()
            .map(|(i, rule)| {
                build_rule(rule).map_err(|problem| ProfileError::Rule {
                    number: i + 1,
                    problem,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            id: raw.id,
            name: raw.name,
            description: raw.description,
            source: raw.source,
            rules,
        })
    }
}

fn build_rule(raw: RawRule) -> Result<Rule, String> {
    let need = |what: &str, v: Option<&str>| {
        v.map(str::to_owned)
            .ok_or_else(|| format!("`{}` needs `{what}`", raw.check))
    };
    let regex = |pattern: &str| Regex::new(pattern).map_err(|e| format!("bad pattern: {e}"));
    let check = match raw.check.as_str() {
        "page_limit" => {
            let pages = raw
                .pages
                .filter(|p| (1..=10_000).contains(p))
                .ok_or("`page_limit` needs `pages` between 1 and 10000")?;
            Check::PageLimit { pages }
        }
        "fonts_embedded" => Check::FontsEmbedded,
        "project_paths_only" => Check::ProjectPathsOnly,
        "portable_file_names" => Check::PortableFileNames {
            allowed: regex(raw.allowed.as_deref().unwrap_or(DEFAULT_FILE_NAME))?,
        },
        "bbl_present" => Check::BblPresent,
        "no_hidden_files" => Check::NoHiddenFiles,
        "unused_files" => Check::UnusedFiles,
        "max_total_size" => {
            let megabytes = raw
                .megabytes
                .filter(|m| *m > 0.0)
                .ok_or("`max_total_size` needs `megabytes` above 0")?;
            Check::MaxTotalSize { megabytes }
        }
        "comments_left" => Check::CommentsLeft,
        "anonymous_authors" => Check::AnonymousAuthors,
        "no_acknowledgements" => Check::NoAcknowledgements,
        "no_self_reference" => Check::NoSelfReference,
        "pdf_metadata_clean" => Check::PdfMetadataClean,
        "alt_text" => Check::AltText,
        "forbid_pattern" => {
            let pattern = need("pattern", raw.pattern.as_deref())?;
            Check::ForbidPattern {
                pattern: regex(&pattern)?,
                message: raw
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("matches `{pattern}`")),
            }
        }
        other => return Err(format!("unknown check `{other}`")),
    };
    let severity = raw.severity.unwrap_or_else(|| check.default_severity());
    Ok(Rule {
        check,
        severity,
        message: raw.message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_read_with_its_rules_and_their_settings() {
        let p = Profile::parse(
            "id = \"v\"\nname = \"V\"\nsource = \"https://x\"\n\
             [[rule]]\ncheck = \"page_limit\"\npages = 8\n\
             [[rule]]\ncheck = \"forbid_pattern\"\npattern = '\\\\todo'\nseverity = \"info\"\n",
        )
        .unwrap();
        assert_eq!(p.rules.len(), 2);
        assert!(matches!(p.rules[0].check, Check::PageLimit { pages: 8 }));
        assert_eq!(p.rules[0].severity, Severity::Error);
        assert_eq!(p.rules[1].severity, Severity::Info);
    }

    #[test]
    fn mistakes_are_named_by_rule_not_swallowed() {
        let err = |body: &str| {
            Profile::parse(&format!("id = \"v\"\nname = \"V\"\n{body}"))
                .unwrap_err()
                .to_string()
        };
        assert!(
            err("[[rule]]\ncheck = \"page_limit\"\n")
                .contains("rule 1: `page_limit` needs `pages`")
        );
        assert!(err("[[rule]]\ncheck = \"nonsense\"\n").contains("unknown check `nonsense`"));
        assert!(
            err("[[rule]]\ncheck = \"forbid_pattern\"\npattern = '('\n").contains("bad pattern")
        );
        assert!(err("[[rule]]\ncheck = \"alt_text\"\nextra = 1\n").contains("not a profile"));
        assert_eq!(
            Profile::parse("id = \"a b\"\nname = \"x\"")
                .unwrap_err()
                .to_string(),
            ProfileError::BadId.to_string()
        );
    }

    #[test]
    fn warnings_are_the_default_for_housekeeping_checks() {
        let p = Profile::parse("id = \"v\"\nname = \"V\"\n[[rule]]\ncheck = \"unused_files\"\n[[rule]]\ncheck = \"fonts_embedded\"\n").unwrap();
        assert_eq!(p.rules[0].severity, Severity::Warning);
        assert_eq!(p.rules[1].severity, Severity::Error);
    }
}
