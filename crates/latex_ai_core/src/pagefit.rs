//! Rules for making a document fit a page limit honestly.
//!
//! Squeezing a paper into N pages is easy if anything goes: shrink the
//! margins, set the text a size smaller, pull the spacing in with a negative
//! `\vspace`. Venues forbid exactly that, and a submission that does it can
//! be rejected unread. So when the goal is a page limit, two things are
//! enforced in code, whatever the model was told:
//!
//! - [`lint_page_fit`]: an edit may not *add* such a trick;
//! - [`Inventory`]: the result may not have lost a section, a figure, a table,
//!   a display equation, a label or a citation key that the original had.
//!
//! Both are heuristics over text, not a TeX parser. They are meant to stop a
//! model that went the easy way, not to defeat someone who tries to evade
//! them; like the rest of [`crate::policy`] they guard against a model going
//! wrong.

use crate::compile::AgentEdit;
use crate::policy::{PolicyViolation, added_text, strip_comment};
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::sync::LazyLock;

/// A pattern for something that buys space dishonestly, and what to call it.
struct Trick {
    what: &'static str,
    pattern: &'static str,
}

const TRICKS: &[Trick] = &[
    Trick {
        what: "a negative \\vspace",
        pattern: r"\\vspace\*?\s*\{\s*-",
    },
    Trick {
        what: "a negative \\vskip or \\hskip",
        pattern: r"\\[vh]skip\s*-",
    },
    Trick {
        what: "a negative line-break skip",
        pattern: r"\\\\\*?\s*\[\s*-",
    },
    Trick {
        what: "changing a page length or spacing parameter",
        pattern: r"\\(?:setlength|addtolength)\s*\{?\s*\\(?:textheight|textwidth|linewidth|oddsidemargin|evensidemargin|topmargin|headheight|headsep|footskip|marginparwidth|columnsep|parskip|parindent|baselineskip|lineskip|abovedisplayskip|belowdisplayskip|abovedisplayshortskip|belowdisplayshortskip|abovecaptionskip|belowcaptionskip|textfloatsep|floatsep|intextsep|dbltextfloatsep|dblfloatsep|topsep|partopsep|itemsep|parsep|columnseprule)\b",
    },
    Trick {
        what: "assigning a page length or spacing parameter",
        pattern: r"\\(?:textheight|textwidth|oddsidemargin|evensidemargin|topmargin|headsep|footskip|columnsep|parskip|baselineskip|lineskip|abovedisplayskip|belowdisplayskip|abovedisplayshortskip|belowdisplayshortskip|abovecaptionskip|belowcaptionskip|textfloatsep|floatsep|intextsep|topsep|partopsep|itemsep|parsep)\s*=\s*[-+0-9.]",
    },
    Trick {
        what: "changing the margins (geometry)",
        pattern: r"\\(?:newgeometry|geometry)\b|\\usepackage\s*(?:\[[^\]]*\])?\s*\{[^}]*\b(?:geometry|fullpage|savetrees|a4wide|vmargin)\b",
    },
    Trick {
        what: "changing the line spacing",
        pattern: r"\\linespread\b|\\baselinestretch\b|\\usepackage\s*(?:\[[^\]]*\])?\s*\{[^}]*\bsetspace\b|\\(?:singlespacing|onehalfspacing|doublespacing|setstretch)\b",
    },
    Trick {
        what: "setting the text in a smaller size",
        pattern: r"\\(?:small|footnotesize|scriptsize|tiny)\b|\\begin\s*\{(?:small|footnotesize|scriptsize|tiny)\}|\\fontsize\b|\\selectfont\b",
    },
    Trick {
        what: "compressing lists or headings",
        pattern: r"\\(?:setlist|setitemize|setenumerate|titlespacing|titleformat)\b|\\usepackage\s*(?:\[[^\]]*\])?\s*\{[^}]*\b(?:enumitem|titlesec|paralist|mdwlist|expdlist)\b",
    },
    Trick {
        what: "stretching a page or a paragraph",
        pattern: r"\\(?:enlargethispage|looseness|raggedbottom|flushbottom)\b",
    },
    Trick {
        what: "changing the document class or its options",
        pattern: r"\\documentclass\b",
    },
];

static COMPILED: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    TRICKS
        .iter()
        .filter_map(|t| Regex::new(t.pattern).ok().map(|re| (t.what, re)))
        .collect()
});

/// Reject `edit` if the text it adds is a space-buying trick.
///
/// Lines already in `original` are exempt: a paper that sets a caption in
/// `\small` may keep doing so; the lint stops the model *introducing* one.
///
/// # Errors
/// The first [`PolicyViolation::LayoutTrick`] found.
pub fn lint_page_fit(edit: &AgentEdit, original: &str) -> Result<(), PolicyViolation> {
    let added = added_text(edit);
    let existing: HashSet<&str> = original.lines().map(str::trim).collect();
    for (i, raw) in added.lines().enumerate() {
        if existing.contains(raw.trim()) {
            continue;
        }
        let line = strip_comment(raw);
        if let Some((what, _)) = COMPILED.iter().find(|(_, re)| re.is_match(line)) {
            return Err(PolicyViolation::LayoutTrick { what, line: i + 1 });
        }
    }
    Ok(())
}

/// What a document is made of, counted so a rewrite can be checked against it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub sections: usize,
    pub figures: usize,
    pub tables: usize,
    /// Display equations: `\[`, `$$` pairs and the numbered environments.
    pub equations: usize,
    pub labels: BTreeSet<String>,
    pub citations: BTreeSet<String>,
}

static SECTION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"\\(?:part|chapter|section|subsection|subsubsection)\*?\s*[\[{]").ok()
});
static FIGURE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"\\includegraphics\b|\\begin\s*\{(?:tikzpicture|pgfpicture)\}").ok()
});
static TABLE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\\begin\s*\{(?:tabular\*?|tabularx|longtable|array)\}").ok());
static EQUATION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"\\begin\s*\{(?:equation|align|gather|multline|eqnarray|flalign|alignat)\*?\}|\\\[|\$\$",
    )
    .ok()
});
static LABEL: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\\label\s*\{([^}]*)\}").ok());
static CITE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"\\[A-Za-z]*cite[A-Za-z]*\*?\s*(?:\[[^\]]*\]\s*)*\{([^}]*)\}").ok()
});

fn count(re: &LazyLock<Option<Regex>>, text: &str) -> usize {
    re.as_ref().map_or(0, |re| re.find_iter(text).count())
}

fn collect(re: &LazyLock<Option<Regex>>, text: &str) -> BTreeSet<String> {
    re.as_ref()
        .into_iter()
        .flat_map(|re| re.captures_iter(text))
        .flat_map(|c| {
            c[1].split(',')
                .map(|k| k.trim().to_owned())
                .filter(|k| !k.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

impl Inventory {
    /// The inventory of `text`, comments ignored.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let live: String = text
            .lines()
            .map(strip_comment)
            .collect::<Vec<_>>()
            .join("\n");
        // `$$` opens and closes a display, so two of them are one equation.
        let dollars = live.matches("$$").count();
        let equations = count(&EQUATION, &live).saturating_sub(dollars) + dollars / 2;
        Self {
            sections: count(&SECTION, &live),
            figures: count(&FIGURE, &live),
            tables: count(&TABLE, &live),
            equations,
            labels: collect(&LABEL, &live),
            citations: collect(&CITE, &live),
        }
    }

    /// What `self` (the original) had that `after` no longer has, in words a
    /// model can act on. Empty when nothing was lost.
    #[must_use]
    pub fn lost_in(&self, after: &Self) -> Vec<String> {
        let mut out = Vec::new();
        let mut fewer = |what: &str, before: usize, now: usize| {
            if now < before {
                out.push(format!("{} {what} removed", before - now));
            }
        };
        fewer("section heading(s)", self.sections, after.sections);
        fewer("figure(s)", self.figures, after.figures);
        fewer("table(s)", self.tables, after.tables);
        fewer("display equation(s)", self.equations, after.equations);
        let names = |gone: BTreeSet<&String>| {
            let mut list: Vec<String> = gone.into_iter().take(6).cloned().collect();
            list.sort();
            list.join(", ")
        };
        let gone: BTreeSet<_> = self.labels.difference(&after.labels).collect();
        if !gone.is_empty() {
            out.push(format!("label(s) removed: {}", names(gone)));
        }
        let gone: BTreeSet<_> = self.citations.difference(&after.citations).collect();
        if !gone.is_empty() {
            out.push(format!("citation(s) removed: {}", names(gone)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::AgentOp;

    fn insert(text: &str) -> AgentEdit {
        AgentEdit {
            file_path: "main.tex".into(),
            op: AgentOp::Insert,
            payload: None,
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some(text.into()),
            expected_etag: None,
            hunk_id: None,
        }
    }

    #[test]
    fn the_usual_ways_of_cheating_are_refused() {
        for bad in [
            r"\vspace{-2mm}",
            r"\vspace*{ -1em }",
            r"\vskip -3pt",
            r"a\\[-4pt]",
            r"\setlength{\textheight}{25cm}",
            r"\addtolength\parskip{-1pt}",
            r"\abovedisplayskip=2pt",
            r"\usepackage[margin=1in]{geometry}",
            r"\usepackage{fullpage}",
            r"\newgeometry{left=1cm}",
            r"\linespread{0.9}",
            r"\renewcommand{\baselinestretch}{0.95}",
            r"\small",
            r"\begin{footnotesize}",
            r"\fontsize{9}{10}\selectfont",
            r"\setlist{nosep}",
            r"\usepackage{titlesec}",
            r"\enlargethispage{2\baselineskip}",
            r"\documentclass[10pt]{article}",
        ] {
            assert!(
                matches!(
                    lint_page_fit(&insert(bad), ""),
                    Err(PolicyViolation::LayoutTrick { .. })
                ),
                "should be refused: {bad}"
            );
        }
    }

    #[test]
    fn honest_edits_pass() {
        for ok in [
            r"\includegraphics[width=0.85\linewidth]{fig.pdf}",
            r"\begin{figure}[t]",
            r"\vspace{2mm}",
            r"We show that the method works.",
            r"\usepackage{microtype}",
            r"\section{Smaller models}",
            r"The \texttt{small} option, and a \smallskip is fine.",
            r"% \vspace{-1cm} commented out",
            r"\\[2pt]",
        ] {
            assert_eq!(lint_page_fit(&insert(ok), ""), Ok(()), "{ok}");
        }
    }

    #[test]
    fn a_trick_already_in_the_paper_may_stay() {
        let original = "\\small\n\\vspace{-1mm}\ntext\n";
        let mut rewrite = insert("");
        rewrite.op = AgentOp::RewriteFile;
        rewrite.payload = Some("\\small\n\\vspace{-1mm}\nshorter text\n".into());
        assert_eq!(lint_page_fit(&rewrite, original), Ok(()));
        rewrite.payload = Some("\\small\n\\vspace{-2mm}\nshorter text\n".into());
        assert!(lint_page_fit(&rewrite, original).is_err());
    }

    #[test]
    fn violations_name_the_trick_and_the_line() {
        let err = lint_page_fit(&insert("fine\n\\vspace{-2mm}\n"), "").unwrap_err();
        assert_eq!(
            err.to_string(),
            "proposed text line 2: a negative \\vspace is not allowed when fitting a page limit; venues ban it"
        );
    }

    const PAPER: &str = "\\section{Intro}\nSee \\cite{a,b} and \\citep[p.~2]{c}.\n\
\\begin{equation}\\label{eq:x} x \\end{equation}\n\
$$ y $$\n\\[ z \\]\n\
\\begin{figure}\\includegraphics{f.png}\\label{fig:f}\\end{figure}\n\
\\begin{tabular}{ll} a & b \\end{tabular}\n\
% \\section{Commented out}\n";

    #[test]
    fn the_inventory_counts_what_a_paper_is_made_of() {
        let inv = Inventory::of(PAPER);
        assert_eq!(inv.sections, 1);
        assert_eq!(inv.figures, 1);
        assert_eq!(inv.tables, 1);
        assert_eq!(inv.equations, 3, "equation env, $$..$$, \\[ \\]");
        assert_eq!(
            inv.citations,
            ["a", "b", "c"].map(String::from).into_iter().collect()
        );
        assert_eq!(
            inv.labels,
            ["eq:x", "fig:f"].map(String::from).into_iter().collect()
        );
    }

    #[test]
    fn rewording_loses_nothing_but_deleting_does() {
        let before = Inventory::of(PAPER);
        let reworded = PAPER.replace("See", "Compare");
        assert_eq!(
            before.lost_in(&Inventory::of(&reworded)),
            Vec::<String>::new()
        );
        // Moving a citation within the text is fine too.
        let moved = PAPER.replace(
            "See \\cite{a,b} and \\citep[p.~2]{c}.",
            "And \\cite{c,b,a}.",
        );
        assert_eq!(before.lost_in(&Inventory::of(&moved)), Vec::<String>::new());

        let cut = PAPER
            .replace("\\section{Intro}\n", "")
            .replace("\\cite{a,b}", "")
            .replace("$$ y $$\n", "")
            .replace("\\begin{tabular}{ll} a & b \\end{tabular}", "");
        let lost = before.lost_in(&Inventory::of(&cut));
        let text = lost.join("; ");
        assert!(text.contains("1 section heading(s) removed"), "{text}");
        assert!(text.contains("1 table(s) removed"), "{text}");
        assert!(text.contains("1 display equation(s) removed"), "{text}");
        assert!(text.contains("citation(s) removed: a, b"), "{text}");
    }

    #[test]
    fn adding_content_is_not_a_loss() {
        let before = Inventory::of("\\section{A}\n");
        let after = Inventory::of("\\section{A}\n\\section{B}\n\\cite{x}\n");
        assert_eq!(before.lost_in(&after), Vec::<String>::new());
    }
}
