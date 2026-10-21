//! What the agent is told when the goal is a page limit.
//!
//! The planner gets measured facts rather than "make it shorter": how far
//! over the document is, the first line that no longer fits, and the
//! paragraphs that end in a word or two (cutting a few words from one of
//! those saves a whole line). The measurements come from [`layout`]; the
//! rules the model must keep to are also enforced in code, by
//! [`latex_ai_core::pagefit`].

use layout::{LayoutReport, Place};
use std::fmt::Write as _;

/// Most short-last-line candidates listed to the model.
const MAX_CANDIDATES: usize = 12;

/// The task as the planner reads it. `extra` is whatever the user typed.
pub(crate) fn instruction(limit: u32, extra: &str) -> String {
    let mut out = format!(
        "Make this document fit within {limit} pages while keeping everything it says.\n\
Allowed, cheapest first: (1) tighten the wording of paragraphs that end in a word or two so they lose a line; \
(2) shorten verbose phrases and repetition in the paragraphs nearest the overflow; \
(3) make figures a little narrower; (4) give floats the [t] placement.\n\
Never: remove or merge sections, figures, tables, equations, labels or citations; \
add a negative \\vspace or \\\\[-..]; change margins, geometry, font size, line spacing or the document class and its options; \
or use \\small, \\footnotesize and the like to shrink text. A submission that does is rejected by the venue.\n\
Edit only what you must: small replacements, not a rewrite of the file."
    );
    let extra = extra.trim();
    if !extra.is_empty() {
        let _ = write!(out, "\nThe author also asks: {extra}");
    }
    out
}

fn spot(p: &Place) -> String {
    format!("{}:{} (page {})", p.file, p.line, p.page)
}

/// How the last attempt measured up, as feedback for the next one.
pub(crate) fn feedback(report: &LayoutReport, limit: u32) -> String {
    let mut out = format!(
        "The document compiles to {} pages; the limit is {limit}.",
        report.pages
    );
    if let Some(budget) = &report.budget {
        let _ = write!(
            out,
            " About {} line{} of text are past it.",
            budget.lines_over,
            if budget.lines_over == 1 { "" } else { "s" }
        );
        if let Some(first) = &budget.first_over {
            let _ = write!(
                out,
                "\nThe first line that no longer fits is at {}.",
                spot(first)
            );
        }
    }
    if report.short_last_lines.is_empty() {
        out.push_str(
            "\nNo paragraph ends in a very short line, so the space has to come from shortening text.",
        );
    } else {
        out.push_str(
            "\nParagraphs whose last line holds only a word or two (the line number is the paragraph's last source line). \
Cutting a few words from one saves a whole line:",
        );
        for p in report.short_last_lines.iter().take(MAX_CANDIDATES) {
            let _ = write!(out, "\n- {}", spot(p));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use layout::{Budget, Fill};

    fn place(page: u32, line: u32) -> Place {
        Place {
            page,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            file: "main.tex".into(),
            line,
        }
    }

    fn report(budget: Option<Budget>, short: Vec<Place>) -> LayoutReport {
        LayoutReport {
            pages: 9,
            fill: Fill {
                last_page_pt: 100.0,
                fullest_page_pt: 600.0,
                ratio: Some(0.16),
            },
            budget,
            boxes: Vec::new(),
            boxes_total: 0,
            short_last_lines_total: short.len(),
            short_last_lines: short,
        }
    }

    #[test]
    fn the_feedback_says_how_far_over_and_where_to_cut() {
        let text = feedback(
            &report(
                Some(Budget {
                    limit: 8,
                    pages: 9,
                    over: true,
                    lines_over: 14,
                    first_over: Some(place(9, 212)),
                }),
                vec![place(3, 40), place(5, 88)],
            ),
            8,
        );
        assert!(
            text.contains("compiles to 9 pages; the limit is 8"),
            "{text}"
        );
        assert!(text.contains("About 14 lines"), "{text}");
        assert!(text.contains("main.tex:212 (page 9)"), "{text}");
        assert!(text.contains("- main.tex:40 (page 3)"), "{text}");
    }

    #[test]
    fn with_no_short_lines_it_says_so() {
        let text = feedback(&report(None, Vec::new()), 8);
        assert!(
            text.contains("No paragraph ends in a very short line"),
            "{text}"
        );
    }

    #[test]
    fn the_task_names_the_limit_the_rules_and_the_authors_words() {
        let text = instruction(6, "keep the abstract as it is");
        assert!(text.contains("within 6 pages"));
        assert!(text.contains("Never: remove or merge sections"));
        assert!(text.ends_with("keep the abstract as it is"));
        assert!(!instruction(6, "  ").contains("also asks"));
    }
}
