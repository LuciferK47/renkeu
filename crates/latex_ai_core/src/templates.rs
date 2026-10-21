//! Starter documents: small `.tex` skeletons for common classes, embedded in
//! the binary so they work offline. The class and style files themselves come
//! from the TeX bundle; only our own skeletons live here.
//!
//! Each template is a folder under `templates/` with a `template.toml`
//! manifest and its files. `main.tex` holds one *front-matter block* between
//! `% renkeu:frontmatter begin` and `% renkeu:frontmatter end`: the title,
//! authors, abstract and the like. Rendering fills `{{title}}`, `{{authors}}`,
//! `{{affiliations}}`, `{{date}}`, `{{abstract}}` and `{{keywords}}` inside it,
//! escaping LaTeX's special characters in the user's text. The begin line also
//! records the template id and the field values as JSON, so the same block can
//! be re-rendered later from a form without parsing LaTeX.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const BEGIN: &str = "% renkeu:frontmatter begin";
const END: &str = "% renkeu:frontmatter end";

/// One embedded template: id, manifest, and `(path, content)` files.
struct Embedded {
    id: &'static str,
    manifest: &'static str,
    files: &'static [(&'static str, &'static str)],
}

macro_rules! template {
    ($id:literal, [$($file:literal),+ $(,)?]) => {
        Embedded {
            id: $id,
            manifest: include_str!(concat!("../templates/", $id, "/template.toml")),
            files: &[$(($file, include_str!(concat!("../templates/", $id, "/", $file)))),+],
        }
    };
}

static TEMPLATES: &[Embedded] = &[
    template!("article", ["main.tex", "refs.bib"]),
    template!("ieee-conf", ["main.tex", "refs.bib"]),
    template!("acm-sigconf", ["main.tex", "refs.bib"]),
    template!("elsevier", ["main.tex", "refs.bib"]),
    template!("springer-lncs", ["main.tex", "refs.bib"]),
    template!("revtex", ["main.tex", "refs.bib"]),
    template!("amsart", ["main.tex", "refs.bib"]),
    template!(
        "thesis",
        ["main.tex", "chapters/introduction.tex", "refs.bib"]
    ),
    template!("lab-report", ["main.tex"]),
    template!("beamer", ["main.tex"]),
    template!("cv", ["main.tex"]),
    template!("homework", ["main.tex"]),
    template!("letter", ["main.tex"]),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    category: String,
    description: String,
    fields: Vec<String>,
    author_item: String,
    author_sep: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Author {
    pub name: String,
    pub affiliation: String,
    pub email: String,
}

/// What the user fills in. Unknown JSON keys are ignored so an older block
/// still loads after a field is added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fields {
    pub title: String,
    pub authors: Vec<Author>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub keywords: String,
    pub date: String,
}

/// A template as the gallery lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateInfo {
    pub id: &'static str,
    pub name: String,
    pub category: String,
    pub description: String,
    /// Which of the form's fields this template uses.
    pub fields: Vec<String>,
}

/// One file to write into a new project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderedFile {
    pub path: &'static str,
    pub content: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("no template called `{0}`")]
    Unknown(String),
    #[error("template `{0}` is damaged: {1}")]
    Broken(String, String),
}

fn find(id: &str) -> Result<(&'static Embedded, Manifest), TemplateError> {
    let t = TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| TemplateError::Unknown(id.to_owned()))?;
    // Compiled in and checked by a test, so this is a build defect.
    let manifest = toml::from_str(t.manifest)
        .map_err(|e| TemplateError::Broken(id.to_owned(), e.to_string()))?;
    Ok((t, manifest))
}

/// Every template, in gallery order.
#[must_use]
pub fn list() -> Vec<TemplateInfo> {
    TEMPLATES
        .iter()
        .filter_map(|t| {
            let (_, m) = find(t.id).ok()?;
            Some(TemplateInfo {
                id: t.id,
                name: m.name,
                category: m.category,
                description: m.description,
                fields: m.fields,
            })
        })
        .collect()
}

/// Escape text for a LaTeX argument. Newlines are kept so paragraphs survive.
#[must_use]
pub fn escape_latex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn apply_author(item: &str, a: &Author) -> String {
    item.replace("{{name}}", &escape_latex(a.name.trim()))
        .replace("{{affiliation}}", &escape_latex(a.affiliation.trim()))
        .replace("{{email}}", &escape_latex(a.email.trim()))
}

fn fill(block: &str, m: &Manifest, f: &Fields) -> String {
    let authors: Vec<&Author> = f
        .authors
        .iter()
        .filter(|a| !a.name.trim().is_empty())
        .collect();
    let authors_tex = authors
        .iter()
        .map(|a| apply_author(&m.author_item, a))
        .collect::<Vec<_>>()
        .join(&m.author_sep);

    let mut seen = BTreeSet::new();
    let affiliations = authors
        .iter()
        .map(|a| a.affiliation.trim())
        .filter(|a| !a.is_empty() && seen.insert(*a))
        .map(escape_latex)
        .collect::<Vec<_>>()
        .join(" \\and ");

    let date = if f.date.trim().is_empty() {
        "\\today".to_owned()
    } else {
        escape_latex(f.date.trim())
    };

    // Values are substituted last and in one pass over distinct tokens, so a
    // title that literally contains "{{abstract}}" is not expanded again.
    let mut out = String::with_capacity(block.len() + 256);
    let mut rest = block;
    while let Some(start) = rest.find("{{") {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let replacement = [
            ("{{title}}", escape_latex(f.title.trim())),
            ("{{authors}}", authors_tex.clone()),
            ("{{affiliations}}", affiliations.clone()),
            ("{{date}}", date.clone()),
            ("{{abstract}}", escape_latex(f.abstract_text.trim())),
            ("{{keywords}}", escape_latex(f.keywords.trim())),
        ]
        .into_iter()
        .find(|(token, _)| tail.starts_with(token));
        if let Some((token, value)) = replacement {
            out.push_str(&value);
            rest = &tail[token.len()..];
        } else {
            // Not a token (`{{{title}}}` starts one brace early): emit one
            // brace and look again from the next.
            out.push('{');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The front-matter block for `template` with `fields`, including its marker
/// lines, ready to replace the block in an existing `main.tex`.
///
/// # Errors
/// [`TemplateError::Unknown`] for an id that is not embedded.
pub fn frontmatter_block(id: &str, fields: &Fields) -> Result<String, TemplateError> {
    let (t, m) = find(id)?;
    let main = t
        .files
        .iter()
        .find(|(p, _)| *p == "main.tex")
        .map_or("", |(_, c)| *c);
    Ok(render_block(main, id, &m, fields))
}

fn render_block(main: &str, id: &str, m: &Manifest, fields: &Fields) -> String {
    let (Some(b), Some(e)) = (main.find(BEGIN), main.find(END)) else {
        return String::new();
    };
    let body_start = main[b..].find('\n').map_or(e, |n| b + n + 1);
    let body = &main[body_start..e];
    // serde_json never emits a raw newline, so the record stays on one line.
    let json = serde_json::to_string(fields).unwrap_or_else(|_| "{}".to_owned());
    format!(
        "{BEGIN} template={id} fields={json}\n{}{END}",
        fill(body, m, fields)
    )
}

/// Every file of `template`, filled in.
///
/// # Errors
/// [`TemplateError::Unknown`] for an id that is not embedded.
pub fn render(id: &str, fields: &Fields) -> Result<Vec<RenderedFile>, TemplateError> {
    let (t, m) = find(id)?;
    Ok(t.files
        .iter()
        .map(|(path, content)| {
            let content = if *path == "main.tex" {
                let b = content.find(BEGIN).unwrap_or(0);
                let e = content.find(END).map_or(content.len(), |e| e + END.len());
                format!(
                    "{}{}{}",
                    &content[..b],
                    render_block(content, id, &m, fields),
                    &content[e..]
                )
            } else {
                (*content).to_owned()
            };
            RenderedFile { path, content }
        })
        .collect())
}

/// A saved block read back from a document: which template, which values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedBlock {
    pub template: String,
    pub fields: Fields,
}

/// Find the front-matter record in `text`, if it has one.
#[must_use]
pub fn read_block(text: &str) -> Option<SavedBlock> {
    let line = text.lines().find(|l| l.starts_with(BEGIN))?;
    let rest = line.strip_prefix(BEGIN)?.trim();
    let rest = rest.strip_prefix("template=")?;
    let (template, json) = rest.split_once(" fields=")?;
    Some(SavedBlock {
        template: template.to_owned(),
        fields: serde_json::from_str(json).ok()?,
    })
}

/// `text` with its front-matter block replaced by `block`, or `None` when it
/// has no block (a hand-written document, say).
#[must_use]
pub fn replace_block(text: &str, block: &str) -> Option<String> {
    let b = text.find(BEGIN)?;
    let e = text[b..].find(END)? + b + END.len();
    Some(format!("{}{}{}", &text[..b], block, &text[e..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Fields {
        Fields {
            title: "Cats & Dogs: 100% of _everything_".to_owned(),
            authors: vec![
                Author {
                    name: "Ada Lovelace".to_owned(),
                    affiliation: "Analytical Engine Co.".to_owned(),
                    email: "ada_l@example.org".to_owned(),
                },
                Author {
                    name: "Alan Turing".to_owned(),
                    affiliation: "Analytical Engine Co.".to_owned(),
                    email: String::new(),
                },
            ],
            abstract_text: "First paragraph.\n\nSecond $paragraph$ with #hash.".to_owned(),
            keywords: "tex, rust".to_owned(),
            date: String::new(),
        }
    }

    #[test]
    fn every_manifest_parses_and_every_template_has_a_front_matter_block() {
        let all = list();
        assert_eq!(all.len(), TEMPLATES.len());
        for t in TEMPLATES {
            let main = t.files.iter().find(|(p, _)| *p == "main.tex").unwrap().1;
            assert!(main.contains(BEGIN), "{} has no begin marker", t.id);
            assert!(main.contains(END), "{} has no end marker", t.id);
            assert!(
                main.contains("\\begin{document}"),
                "{} must start the document inside the block or after it",
                t.id
            );
        }
    }

    #[test]
    fn user_text_is_escaped() {
        let files = render("article", &sample()).unwrap();
        let main = &files.iter().find(|f| f.path == "main.tex").unwrap().content;
        assert!(main.contains(r"\title{Cats \& Dogs: 100\% of \_everything\_}"));
        assert!(main.contains(r"Second \$paragraph\$ with \#hash."));
        assert!(main.contains(r"\texttt{ada\_l@example.org}"));
        assert!(
            !main.contains("{{"),
            "an unfilled placeholder is left: {main}"
        );
    }

    #[test]
    fn a_value_that_looks_like_a_placeholder_is_not_expanded() {
        let mut f = sample();
        f.title = "{{abstract}}".to_owned();
        let files = render("article", &f).unwrap();
        let main = &files[0].content;
        assert!(main.contains(r"\title{\{\{abstract\}\}}"));
    }

    #[test]
    fn the_record_round_trips_and_replaces_only_the_block() {
        let first = render("ieee-conf", &sample()).unwrap();
        let main = first[0].content.clone();
        let saved = read_block(&main).expect("block");
        assert_eq!(saved.template, "ieee-conf");
        assert_eq!(saved.fields, sample());

        let mut changed = sample();
        changed.title = "A new title".to_owned();
        let block = frontmatter_block("ieee-conf", &changed).unwrap();
        let edited = replace_block(&main, &block).unwrap();
        assert!(edited.contains(r"\title{A new title}"));
        // Everything outside the block is untouched.
        let before = &main[..main.find(BEGIN).unwrap()];
        let after = &main[main.find(END).unwrap() + END.len()..];
        assert!(edited.starts_with(before));
        assert!(edited.ends_with(after));
        assert_eq!(read_block(&edited).unwrap().fields, changed);
    }

    #[test]
    fn authors_without_a_name_are_skipped_and_affiliations_are_deduplicated() {
        let mut f = sample();
        f.authors.push(Author::default());
        let files = render("springer-lncs", &f).unwrap();
        let main = &files[0].content;
        assert!(main.contains(r"\author{Ada Lovelace \and Alan Turing}"));
        assert!(main.contains(r"\institute{Analytical Engine Co.}"));
    }

    #[test]
    fn an_empty_date_means_today() {
        let files = render("article", &sample()).unwrap();
        assert!(files[0].content.contains(r"\date{\today}"));
    }

    #[test]
    fn an_unknown_template_is_an_error() {
        assert_eq!(
            render("nope", &Fields::default()),
            Err(TemplateError::Unknown("nope".to_owned()))
        );
    }

    #[test]
    fn escaping_covers_every_special_character() {
        assert_eq!(
            escape_latex(r"\ ~ ^ & % $ # _ { }"),
            r"\textbackslash{} \textasciitilde{} \textasciicircum{} \& \% \$ \# \_ \{ \}"
        );
    }
}
