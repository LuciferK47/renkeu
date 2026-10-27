//! Each check against projects that pass and projects that do not.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use preflight::{Facts, FileInfo, Profile, Project, Report, Severity};

fn project(files: &[(&str, &str)]) -> Project {
    Project {
        main: "main.tex".into(),
        files: files
            .iter()
            .map(|(name, text)| {
                (
                    (*name).to_owned(),
                    FileInfo {
                        size: text.len() as u64,
                        text: [".tex", ".bib", ".sty", ".bbl"]
                            .iter()
                            .any(|e| name.ends_with(e))
                            .then(|| (*text).to_owned()),
                    },
                )
            })
            .collect(),
    }
}

fn run(rules: &str, project: &Project, facts: &Facts) -> Report {
    let profile = Profile::parse(&format!("id = \"t\"\nname = \"T\"\n{rules}")).unwrap();
    preflight::run(&profile, project, facts)
}

fn messages(report: &Report) -> Vec<String> {
    report.findings.iter().map(|f| f.message.clone()).collect()
}

const ONE: &str = "[[rule]]\ncheck = ";

#[test]
fn the_page_limit_compares_the_build_with_the_number_in_the_profile() {
    let p = project(&[("main.tex", "x")]);
    let rule = format!("{ONE}\"page_limit\"\npages = 8\n");
    let over = run(
        &rule,
        &p,
        &Facts {
            pages: Some(9),
            ..Facts::default()
        },
    );
    assert!(!over.passed);
    assert_eq!(messages(&over), ["9 pages, the limit is 8"]);
    assert!(
        run(
            &rule,
            &p,
            &Facts {
                pages: Some(8),
                ..Facts::default()
            }
        )
        .passed
    );
    // Not built: not a pass, and not a failure either.
    let unknown = run(&rule, &p, &Facts::default());
    assert!(unknown.passed);
    assert_eq!(unknown.findings[0].severity, Severity::Info);
}

#[test]
fn fonts_and_pdf_metadata_come_from_the_build() {
    let p = project(&[("main.tex", "x")]);
    let facts = Facts {
        fonts_not_embedded: Some(vec!["Helvetica".into()]),
        pdf_author: Some("Ada Lovelace".into()),
        ..Facts::default()
    };
    let r = run(
        &format!("{ONE}\"fonts_embedded\"\n{ONE}\"pdf_metadata_clean\"\n"),
        &p,
        &facts,
    );
    assert_eq!(
        messages(&r),
        [
            "font Helvetica is not embedded",
            "the PDF's Author field is \"Ada Lovelace\""
        ]
    );
    let clean = Facts {
        fonts_not_embedded: Some(vec![]),
        pdf_author: Some(String::new()),
        ..Facts::default()
    };
    assert_eq!(
        run(
            &format!("{ONE}\"fonts_embedded\"\n{ONE}\"pdf_metadata_clean\"\n"),
            &p,
            &clean
        )
        .findings,
        vec![]
    );
}

#[test]
fn paths_that_leave_the_project_are_named_with_their_line() {
    let p = project(&[(
        "main.tex",
        "\\input{ok/a}\n\\includegraphics{/home/me/fig.png}\n\\graphicspath{{figs/}{../shared/}}\n% \\input{/commented}\n",
    )]);
    let r = run(
        &format!("{ONE}\"project_paths_only\"\n"),
        &p,
        &Facts::default(),
    );
    let found: Vec<_> = r
        .findings
        .iter()
        .map(|f| (f.line.unwrap(), f.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                2,
                "`\\includegraphics` reaches outside the project: /home/me/fig.png"
            ),
            (
                3,
                "`\\graphicspath` reaches outside the project: ../shared/"
            )
        ]
    );
}

#[test]
fn file_names_and_hidden_files_follow_the_profile() {
    let p = project(&[
        ("main.tex", ""),
        ("my figure.png", ""),
        ("figs/ok_1+2.png", ""),
        (".env", ""),
        (".git/", ""),
    ]);
    let r = run(
        &format!("{ONE}\"portable_file_names\"\n{ONE}\"no_hidden_files\"\n"),
        &p,
        &Facts::default(),
    );
    let m = messages(&r);
    assert!(m.iter().any(|x| x.contains("\"my figure.png\"")), "{m:?}");
    assert!(m.iter().any(|x| x.starts_with(".env is a hidden")), "{m:?}");
    assert!(
        m.iter().any(|x| x.starts_with(".git/ is a hidden")),
        "{m:?}"
    );
    assert!(!m.iter().any(|x| x.contains("ok_1+2")), "{m:?}");
    let strict = run(
        &format!("{ONE}\"portable_file_names\"\nallowed = '^[a-z.]+$'\n"),
        &p,
        &Facts::default(),
    );
    assert!(strict.findings.len() >= 2);
}

#[test]
fn a_bibtex_project_needs_the_bbl_named_after_the_main_file() {
    let rule = format!("{ONE}\"bbl_present\"\n");
    let without = project(&[("main.tex", "\\bibliography{refs}"), ("refs.bib", "")]);
    let r = run(
        &rule,
        &without,
        &Facts {
            built_bbl: true,
            ..Facts::default()
        },
    );
    assert!(
        r.findings[0]
            .message
            .contains("no main.bbl; the build produced one, add it")
    );
    assert!(
        run(
            &rule,
            &project(&[("main.tex", "\\bibliography{refs}"), ("main.bbl", "")]),
            &Facts::default()
        )
        .passed
    );
    assert_eq!(
        run(
            &rule,
            &project(&[("main.tex", "no bibliography here")]),
            &Facts::default()
        )
        .findings,
        vec![]
    );
}

#[test]
fn unused_files_are_those_the_document_never_names() {
    let p = project(&[
        (
            "main.tex",
            "\\documentclass{article}\\usepackage{mine}\\input{chap/one}\\includegraphics{fig}\\includegraphics{sub/fig2.png}\\bibliography{refs}\\graphicspath{{img/}}\\includegraphics{pic}",
        ),
        ("chap/one.tex", "\\includegraphics{diagram.pdf}"),
        ("fig.pdf", ""),
        ("sub/fig2.png", ""),
        ("img/pic.png", ""),
        ("diagram.pdf", ""),
        ("mine.sty", ""),
        ("refs.bib", ""),
        ("main.bbl", ""),
        ("old/backup.tex", ""),
        ("notes.txt", ""),
    ]);
    let r = run(&format!("{ONE}\"unused_files\"\n"), &p, &Facts::default());
    let mut unused: Vec<_> = r.findings.iter().map(|f| f.file.clone().unwrap()).collect();
    unused.sort();
    assert_eq!(unused, ["notes.txt", "old/backup.tex"]);
    assert!(r.passed, "unused files are a warning by default");
}

#[test]
fn comments_are_counted_but_editor_directives_are_not() {
    let p = project(&[(
        "main.tex",
        "% !TEX root = main.tex\ntext % a note\n% another\n50\\% fine\n%\n",
    )]);
    let r = run(&format!("{ONE}\"comments_left\"\n"), &p, &Facts::default());
    assert_eq!(messages(&r), ["2 comment line(s) are still in the source"]);
    assert_eq!(r.findings[0].line, Some(2));
}

#[test]
fn authors_emails_and_metadata_that_identify_are_found_and_anonymous_ones_are_not() {
    let rule = format!("{ONE}\"anonymous_authors\"\n");
    let anon = project(&[(
        "main.tex",
        "\\author{Anonymous Authors}\n\\affiliation{}\n\\hypersetup{pdfauthor={Anonymous}}\n",
    )]);
    assert_eq!(run(&rule, &anon, &Facts::default()).findings, vec![]);

    let named = project(&[(
        "main.tex",
        "\\author{Ada Lovelace\\thanks{x}}\n\\email{ada@example.org}\n\\hypersetup{pdftitle={T},pdfauthor={Ada}}\nCode at https://github.com/ada/tool\n",
    )]);
    let r = run(&rule, &named, &Facts::default());
    let m = messages(&r);
    assert!(
        m.iter().any(|x| x == "`\\author` names: Ada Lovelace x"),
        "{m:?}"
    );
    assert!(m.iter().any(|x| x.starts_with("`\\email` names")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("pdfauthor")), "{m:?}");
    assert!(
        m.iter()
            .any(|x| x.contains("email address: ada@example.org")),
        "{m:?}"
    );
    let github = r
        .findings
        .iter()
        .find(|f| f.message.contains("github.com/ada"))
        .unwrap();
    assert_eq!(github.severity, Severity::Warning);
    assert!(!r.passed);
}

#[test]
fn acknowledgements_in_all_their_usual_forms() {
    let p = project(&[(
        "main.tex",
        "\\thanks{Funded by X}\n\\begin{acknowledgments}\\end{acknowledgments}\n\\section*{Acknowledgements}\n\\section{Funding}\n\\section{Results}\n",
    )]);
    let r = run(
        &format!("{ONE}\"no_acknowledgements\"\n"),
        &p,
        &Facts::default(),
    );
    let lines: Vec<_> = r.findings.iter().map(|f| f.line.unwrap()).collect();
    assert_eq!(lines, [1, 2, 3, 4]);
}

#[test]
fn sentences_that_point_back_at_the_authors_are_flagged_as_a_warning() {
    let p = project(&[(
        "main.tex",
        "As shown in our previous work, it holds.\nWe previously proposed X.\nThe method of Smith et al. is older.\n",
    )]);
    let r = run(
        &format!("{ONE}\"no_self_reference\"\n"),
        &p,
        &Facts::default(),
    );
    assert_eq!(r.findings.len(), 2);
    assert!(r.passed, "a heuristic is a warning, not a failure");
}

#[test]
fn images_without_alt_text_are_listed_where_they_are() {
    let p = project(&[(
        "main.tex",
        "\\includegraphics[width=3cm]{a.png}\n\\includegraphics[alt={A cat},width=3cm]{b.png}\n\\includegraphics{c.png}\n",
    )]);
    let r = run(&format!("{ONE}\"alt_text\"\n"), &p, &Facts::default());
    let lines: Vec<_> = r.findings.iter().map(|f| f.line.unwrap()).collect();
    assert_eq!(lines, [1, 3]);
}

#[test]
fn a_profile_can_forbid_any_pattern_and_say_why() {
    let p = project(&[("main.tex", "ok\n\\todo{fix}\n% \\todo{commented}\n")]);
    let r = run(
        &format!(
            "{ONE}\"forbid_pattern\"\npattern = '\\\\todo'\nmessage = \"No todo notes in the final version.\"\n"
        ),
        &p,
        &Facts::default(),
    );
    assert_eq!(messages(&r), ["No todo notes in the final version."]);
    assert_eq!(r.findings[0].line, Some(2));
}

#[test]
fn a_rule_can_lower_but_never_raise_a_finding() {
    let p = project(&[("main.tex", "\\author{Ada}\n")]);
    let lowered = run(
        &format!("{ONE}\"anonymous_authors\"\nseverity = \"warning\"\n"),
        &p,
        &Facts::default(),
    );
    assert!(lowered.passed);
    assert_eq!(lowered.findings[0].severity, Severity::Warning);
    let alt = project(&[("main.tex", "\\includegraphics{a.png}\n")]);
    let raised = run(
        &format!("{ONE}\"alt_text\"\nseverity = \"error\"\n"),
        &alt,
        &Facts::default(),
    );
    assert_eq!(
        raised.findings[0].severity,
        Severity::Warning,
        "the check decides how bad it can be"
    );
}

#[test]
fn a_long_list_is_cut_and_says_how_much_was_left_out() {
    let files: Vec<(String, &str)> = (0..40).map(|i| (format!("junk{i}.txt"), "")).collect();
    let mut all: Vec<(&str, &str)> = vec![("main.tex", "")];
    all.extend(files.iter().map(|(n, t)| (n.as_str(), *t)));
    let r = run(
        &format!("{ONE}\"unused_files\"\n"),
        &project(&all),
        &Facts::default(),
    );
    assert_eq!(r.findings.len(), 26);
    assert_eq!(r.findings.last().unwrap().message, "... and 15 more");
}

#[test]
fn the_shipped_arxiv_profile_passes_a_clean_project_and_catches_the_usual_mistakes() {
    let arxiv = preflight::built_in()
        .into_iter()
        .find(|p| p.id == "arxiv")
        .unwrap();
    let clean = project(&[(
        "main.tex",
        "\\documentclass{article}\\begin{document}Hi\\end{document}",
    )]);
    assert_eq!(
        preflight::run(&arxiv, &clean, &Facts::default()).findings,
        vec![]
    );

    let messy = project(&[
        (
            "main.tex",
            "\\documentclass{article}\\date{\\today}\\bibliography{r}\\begin{document}\\end{document}",
        ),
        ("r.bib", ""),
        ("Figure 1.png", ""),
        (".DS_Store", ""),
    ]);
    let r = preflight::run(&arxiv, &messy, &Facts::default());
    let m = messages(&r).join("\n");
    for needle in [
        "a-z A-Z 0-9",
        "hidden files",
        "pre-generated .bbl",
        "not used by the document",
        "\\today",
    ] {
        assert!(m.contains(needle), "missing {needle:?} in:\n{m}");
    }
    assert!(!r.passed);
}

#[test]
fn reading_a_project_from_disk_skips_links_and_lists_git_as_one_hidden_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.tex"), "\\input{a}").unwrap();
    std::fs::write(dir.path().join("a.tex"), "x").unwrap();
    std::fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
    std::fs::write(dir.path().join(".git/objects/blob"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("link.tex")).unwrap();
    let p = Project::read(dir.path(), "main.tex").unwrap();
    let names: Vec<_> = p.files.keys().cloned().collect();
    assert_eq!(names, [".git/", "a.tex", "main.tex"]);
    assert_eq!(p.files["main.tex"].text.as_deref(), Some("\\input{a}"));
}
