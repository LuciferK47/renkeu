//! The checks themselves. Each takes the project and the build's facts and
//! returns findings at the highest severity it would ever give; the profile's
//! rule can only lower that.

use crate::profile::{Check, Severity};
use crate::tex::{Cmd, commands, live, plain};
use crate::{Facts, Finding, Project};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::sync::LazyLock;

/// Most findings one check lists; the rest are counted in a last finding.
const LIST_CAP: usize = 25;

/// The 1-based line number for a 0-based index.
fn line_no(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

fn finding(
    check: &Check,
    severity: Severity,
    message: impl Into<String>,
    file: Option<&str>,
    line: Option<u32>,
) -> Finding {
    Finding {
        check: check.id(),
        severity,
        message: message.into(),
        file: file.map(str::to_owned),
        line,
    }
}

/// Cap a long list, saying how many were left out.
fn capped(check: &Check, severity: Severity, mut list: Vec<Finding>) -> Vec<Finding> {
    if list.len() > LIST_CAP {
        let more = list.len() - LIST_CAP;
        list.truncate(LIST_CAP);
        list.push(finding(
            check,
            severity,
            format!("... and {more} more"),
            None,
            None,
        ));
    }
    list
}

pub(crate) fn run(check: &Check, project: &Project, facts: &Facts) -> Vec<Finding> {
    match check {
        Check::PageLimit { pages } => page_limit(check, *pages, facts),
        Check::FontsEmbedded => fonts(check, facts),
        Check::ProjectPathsOnly => project_paths(check, project),
        Check::PortableFileNames { allowed } => file_names(check, allowed, project),
        Check::BblPresent => bbl(check, project, facts),
        Check::NoHiddenFiles => hidden(check, project),
        Check::UnusedFiles => unused(check, project),
        Check::MaxTotalSize { megabytes } => total_size(check, *megabytes, project),
        Check::CommentsLeft => comments(check, project),
        Check::AnonymousAuthors => authors(check, project),
        Check::NoAcknowledgements => acknowledgements(check, project),
        Check::NoSelfReference => self_reference(check, project),
        Check::PdfMetadataClean => pdf_metadata(check, facts),
        Check::AltText => alt_text(check, project),
        Check::ForbidPattern { pattern, message } => forbid(check, pattern, message, project),
    }
}

fn page_limit(check: &Check, limit: u32, facts: &Facts) -> Vec<Finding> {
    match facts.pages {
        None => vec![finding(
            check,
            Severity::Info,
            "the page count is unknown because the document has not been built",
            None,
            None,
        )],
        Some(n) if n > limit => vec![finding(
            check,
            Severity::Error,
            format!("{n} pages, the limit is {limit}"),
            None,
            None,
        )],
        Some(_) => Vec::new(),
    }
}

fn fonts(check: &Check, facts: &Facts) -> Vec<Finding> {
    match &facts.fonts_not_embedded {
        None => vec![finding(
            check,
            Severity::Info,
            "the fonts could not be inspected (libpdfium was not found)",
            None,
            None,
        )],
        Some(list) => list
            .iter()
            .map(|f| {
                finding(
                    check,
                    Severity::Error,
                    format!("font {f} is not embedded"),
                    None,
                    None,
                )
            })
            .collect(),
    }
}

/// Commands whose argument names a file.
const PATH_COMMANDS: &[&str] = &[
    "input",
    "include",
    "includegraphics",
    "includepdf",
    "includesvg",
    "bibliography",
    "addbibresource",
    "lstinputlisting",
    "verbatiminput",
    "usepackage",
    "RequirePackage",
    "documentclass",
    "subfile",
    "import",
    "subimport",
];

fn escapes_project(path: &str) -> bool {
    let b = path.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    path.starts_with(['/', '\\', '~', '|']) || drive || path.split(['/', '\\']).any(|p| p == "..")
}

/// The `{...}` groups inside a `\graphicspath{{a/}{b/}}` argument.
fn groups(arg: &str) -> Vec<String> {
    let (mut out, mut depth, mut cur) = (Vec::new(), 0, String::new());
    for c in arg.chars() {
        match c {
            '{' => {
                depth += 1;
                if depth > 1 {
                    cur.push(c);
                }
            }
            '}' => {
                depth -= 1;
                if depth >= 1 {
                    cur.push(c);
                } else if depth == 0 {
                    // A top-level group has no wrapper; the group of groups does.
                }
                if depth == 0 && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c if depth >= 1 => cur.push(c),
            _ => {}
        }
    }
    out
}

fn project_paths(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for cmd in commands(text) {
            let paths: Vec<String> = if cmd.name == "graphicspath" {
                cmd.args.first().map(|a| groups(a)).unwrap_or_default()
            } else if PATH_COMMANDS.contains(&cmd.name.as_str()) {
                cmd.args
                    .iter()
                    .flat_map(|a| a.split(','))
                    .map(|p| p.trim().to_owned())
                    .collect()
            } else {
                continue;
            };
            for path in paths.iter().filter(|p| !p.is_empty() && escapes_project(p)) {
                out.push(finding(
                    check,
                    Severity::Error,
                    format!("`\\{}` reaches outside the project: {path}", cmd.name),
                    Some(file),
                    Some(cmd.line),
                ));
            }
        }
    }
    capped(check, Severity::Error, out)
}

fn file_names(check: &Check, allowed: &Regex, project: &Project) -> Vec<Finding> {
    let out = project
        .files
        .keys()
        .filter(|p| !p.ends_with('/') && !allowed.is_match(p))
        .map(|p| {
            finding(
                check,
                Severity::Error,
                format!("the file name {p:?} has characters that are not allowed"),
                Some(p),
                None,
            )
        })
        .collect();
    capped(check, Severity::Error, out)
}

fn stem(main: &str) -> &str {
    main.strip_suffix(".tex").unwrap_or(main)
}

fn bbl(check: &Check, project: &Project, facts: &Facts) -> Vec<Finding> {
    let uses_bibtex = project
        .text_files()
        .filter(|(p, _)| Project::is_tex(p))
        .any(|(_, text)| {
            commands(text)
                .iter()
                .any(|c| matches!(c.name.as_str(), "bibliography" | "addbibresource"))
        });
    let wanted = format!("{}.bbl", stem(&project.main));
    if !uses_bibtex || project.files.contains_key(&wanted) {
        return Vec::new();
    }
    let extra = if facts.built_bbl {
        "; the build produced one, add it to the project"
    } else {
        ""
    };
    vec![finding(
        check,
        Severity::Error,
        format!("the bibliography is built from .bib files but the project has no {wanted}{extra}"),
        Some(&project.main),
        None,
    )]
}

fn hidden(check: &Check, project: &Project) -> Vec<Finding> {
    let out = project
        .files
        .keys()
        .filter(|p| {
            p.trim_end_matches('/')
                .split('/')
                .any(|part| part.starts_with('.'))
        })
        .map(|p| {
            finding(
                check,
                Severity::Error,
                format!("{p} is a hidden file or folder"),
                Some(p),
                None,
            )
        })
        .collect();
    capped(check, Severity::Error, out)
}

fn total_size(check: &Check, megabytes: f64, project: &Project) -> Vec<Finding> {
    #[allow(clippy::cast_precision_loss)]
    let total = project.files.values().map(|f| f.size).sum::<u64>() as f64 / 1_000_000.0;
    if total > megabytes {
        vec![finding(
            check,
            Severity::Error,
            format!("the project is {total:.1} MB, over {megabytes} MB"),
            None,
            None,
        )]
    } else {
        Vec::new()
    }
}

fn comments(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        let mut count = 0;
        let mut first = None;
        for (i, line) in text.lines().enumerate() {
            let rest = &line[live(line).len()..];
            // `% !TEX root = ...` is a note to the editor, not a comment to hide.
            if rest.trim_start_matches('%').trim().is_empty()
                || rest
                    .trim_start_matches('%')
                    .trim_start()
                    .starts_with("!TEX")
            {
                continue;
            }
            count += 1;
            first.get_or_insert(line_no(i));
        }
        if count > 0 {
            out.push(finding(
                check,
                Severity::Warning,
                format!("{count} comment line(s) are still in the source"),
                Some(file),
                first,
            ));
        }
    }
    out
}

// ---------------------------------------------------------------- unused files

const GRAPHIC_EXTENSIONS: &[&str] = &["pdf", "png", "jpg", "jpeg", "eps", "svg", "ps", "gif"];

fn normalise(path: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Project files the main document reaches, by following what it names.
fn reachable(project: &Project) -> BTreeSet<String> {
    let base = Path::new(&project.main)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut graphics_dirs: Vec<String> = vec![String::new()];
    for (_, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for cmd in commands(text).iter().filter(|c| c.name == "graphicspath") {
            graphics_dirs.extend(cmd.args.first().map(|a| groups(a)).unwrap_or_default());
        }
    }

    let mut seen = BTreeSet::new();
    let mut queue = vec![project.main.clone()];
    while let Some(file) = queue.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let Some(text) = project.files.get(&file).and_then(|f| f.text.as_ref()) else {
            continue;
        };
        for cmd in commands(text) {
            for name in references(&cmd, &graphics_dirs) {
                let Some(path) = normalise(&base.join(&name)) else {
                    continue;
                };
                if project.files.contains_key(&path) && !seen.contains(&path) {
                    queue.push(path);
                }
            }
        }
    }
    seen.insert(format!("{}.bbl", stem(&project.main)));
    seen
}

/// File names a command may mean, before looking for them on disk.
fn references(cmd: &Cmd, graphics_dirs: &[String]) -> Vec<String> {
    let first = cmd
        .args
        .first()
        .map(|a| a.trim().to_owned())
        .unwrap_or_default();
    let with_ext = |name: &str, exts: &[&str]| -> Vec<String> {
        let mut out = vec![name.to_owned()];
        if Path::new(name).extension().is_none() {
            out.extend(exts.iter().map(|e| format!("{name}.{e}")));
        }
        out
    };
    let each = |exts: &[&str]| -> Vec<String> {
        first
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .flat_map(|n| with_ext(n, exts))
            .collect()
    };
    match cmd.name.as_str() {
        "input" | "include" | "subfile" => with_ext(&first, &["tex"]),
        "import" | "subimport" => {
            let file = cmd.args.get(1).map(|a| a.trim()).unwrap_or_default();
            with_ext(&format!("{}/{file}", first.trim_end_matches('/')), &["tex"])
        }
        "includegraphics" | "includepdf" | "includesvg" => graphics_dirs
            .iter()
            .flat_map(|d| with_ext(&format!("{d}{first}"), GRAPHIC_EXTENSIONS))
            .collect(),
        "bibliography" | "addbibresource" | "addglobalbib" => each(&["bib"]),
        "bibliographystyle" => with_ext(&first, &["bst"]),
        "usepackage" | "RequirePackage" => each(&["sty"]),
        "documentclass" | "LoadClass" => with_ext(&first, &["cls"]),
        "lstinputlisting" | "verbatiminput" => vec![first],
        _ => Vec::new(),
    }
}

fn unused(check: &Check, project: &Project) -> Vec<Finding> {
    let reached = reachable(project);
    let out = project
        .files
        .keys()
        .filter(|p| !p.ends_with('/') && !reached.contains(*p))
        .filter(|p| {
            !p.rsplit('/')
                .next()
                .is_some_and(|n| n.starts_with('.') || n.starts_with("00README"))
        })
        .map(|p| {
            finding(
                check,
                Severity::Warning,
                format!("{p} is not used by the document"),
                Some(p),
                None,
            )
        })
        .collect();
    capped(check, Severity::Warning, out)
}

// ------------------------------------------------------------------ anonymity

static ANONYMOUS: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)^(|anonymous( authors?)?|anon\.?|authors? (omitted|hidden|withheld)|double[- ]blind|submitted for (blind )?review|\.{3}|…)$").ok()
});
static EMAIL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}").ok()
});
static GITHUB: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"github\.com/[A-Za-z0-9][A-Za-z0-9-]*").ok());

fn is_anonymous(text: &str) -> bool {
    ANONYMOUS
        .as_ref()
        .is_some_and(|re| re.is_match(text.trim()))
}

const IDENTITY_COMMANDS: &[&str] = &[
    "author",
    "IEEEauthorblockN",
    "IEEEauthorblockA",
    "affiliation",
    "institute",
    "address",
    "email",
    "orcid",
    "authorrunning",
    "institution",
];

fn authors(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for cmd in commands(text) {
            let name = cmd.name.as_str();
            if IDENTITY_COMMANDS.contains(&name) {
                let shown = plain(cmd.args.last().map_or("", String::as_str));
                if !is_anonymous(&shown) {
                    let short: String = shown.chars().take(50).collect();
                    out.push(finding(
                        check,
                        Severity::Error,
                        format!("`\\{name}` names: {short}"),
                        Some(file),
                        Some(cmd.line),
                    ));
                }
            } else if name == "hypersetup" && cmd.args.first().is_some_and(|a| has_pdfauthor(a)) {
                out.push(finding(
                    check,
                    Severity::Error,
                    "`pdfauthor` puts the authors in the PDF's metadata",
                    Some(file),
                    Some(cmd.line),
                ));
            }
        }
        for (i, line) in text.lines().enumerate() {
            let code = live(line);
            let at = |m: &str, sev, what: &str| {
                finding(
                    check,
                    sev,
                    format!("{what}: {m}"),
                    Some(file),
                    Some(line_no(i)),
                )
            };
            if let Some(m) = EMAIL.as_ref().and_then(|re| re.find(code)) {
                out.push(at(m.as_str(), Severity::Error, "email address"));
            }
            if let Some(m) = GITHUB.as_ref().and_then(|re| re.find(code)) {
                out.push(at(
                    m.as_str(),
                    Severity::Warning,
                    "a link that may point at the authors",
                ));
            }
        }
    }
    capped(check, Severity::Error, out)
}

fn has_pdfauthor(arg: &str) -> bool {
    arg.split(',').any(|kv| {
        kv.split_once('=')
            .is_some_and(|(k, v)| k.trim() == "pdfauthor" && !is_anonymous(&plain(v)))
    })
}

fn acknowledgements(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for cmd in commands(text) {
            let first = cmd
                .args
                .first()
                .map_or(String::new(), |a| plain(a).to_lowercase());
            let hit = match cmd.name.as_str() {
                "thanks" => Some("`\\thanks` names or thanks someone"),
                "acknowledgments" | "acknowledgements" | "acknowledgment" | "acknowledgement" => {
                    Some("an acknowledgements command")
                }
                "begin" if first.starts_with("ack") => Some("an acknowledgements environment"),
                "section" | "subsection" | "chapter" | "paragraph" | "subsubsection"
                    if first.starts_with("acknowledg") || first.starts_with("funding") =>
                {
                    Some("an acknowledgements or funding section")
                }
                _ => None,
            };
            if let Some(what) = hit {
                out.push(finding(
                    check,
                    Severity::Error,
                    what,
                    Some(file),
                    Some(cmd.line),
                ));
            }
        }
    }
    out
}

static SELF_REFERENCE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\b(?:our|my)\s+(?:previous|earlier|prior|past|recent)\s+(?:work|paper|papers|study|studies|results?|publications?)",
        r"(?i)\bwe\s+(?:have\s+)?(?:previously|earlier|recently)\s+(?:shown|showed|proposed|introduced|presented|reported|developed|published)",
        r"(?i)\bin\s+our\s+(?:previous|earlier|prior)\b",
        r"(?i)\bthe\s+authors'\s+(?:previous|earlier|prior)\b",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

fn self_reference(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for (i, line) in text.lines().enumerate() {
            let code = live(line);
            if let Some(m) = SELF_REFERENCE.iter().find_map(|re| re.find(code)) {
                out.push(finding(
                    check,
                    Severity::Warning,
                    format!("reads as the authors citing themselves: \"{}\"", m.as_str()),
                    Some(file),
                    Some(line_no(i)),
                ));
            }
        }
    }
    capped(check, Severity::Warning, out)
}

fn pdf_metadata(check: &Check, facts: &Facts) -> Vec<Finding> {
    match &facts.pdf_author {
        None => vec![finding(
            check,
            Severity::Info,
            "the PDF's metadata could not be read",
            None,
            None,
        )],
        Some(a) if a.trim().is_empty() => Vec::new(),
        Some(a) => vec![finding(
            check,
            Severity::Error,
            format!("the PDF's Author field is \"{a}\""),
            None,
            None,
        )],
    }
}

fn alt_text(check: &Check, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for cmd in commands(text)
            .iter()
            .filter(|c| c.name == "includegraphics")
        {
            let described = cmd
                .options
                .as_deref()
                .is_some_and(|o| o.split(',').any(|kv| kv.trim_start().starts_with("alt")));
            if !described {
                let name = cmd.args.first().map_or("", String::as_str);
                out.push(finding(
                    check,
                    Severity::Warning,
                    format!("the image {name} has no alt text"),
                    Some(file),
                    Some(cmd.line),
                ));
            }
        }
    }
    capped(check, Severity::Warning, out)
}

fn forbid(check: &Check, pattern: &Regex, message: &str, project: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    for (file, text) in project.text_files().filter(|(p, _)| Project::is_tex(p)) {
        for (i, line) in text.lines().enumerate() {
            if pattern.is_match(live(line)) {
                out.push(finding(
                    check,
                    Severity::Error,
                    message,
                    Some(file),
                    Some(line_no(i)),
                ));
            }
        }
    }
    capped(check, Severity::Error, out)
}
