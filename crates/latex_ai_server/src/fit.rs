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
use visual::render::BodyMetrics;

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

/// Smaller than this share of the original type size counts as shrinking it.
const FONT_TOLERANCE: f32 = 0.985;
/// Line spacing closer than this share of the original counts as tightening it.
const PITCH_TOLERANCE: f32 = 0.98;
/// A text block this much bigger than the original has had its margins cut.
const BLOCK_TOLERANCE: f32 = 1.02;
/// Cutting more than this share of the text is deleting, not tightening.
const KEEP_AT_LEAST: f32 = 0.6;

/// What the compiled PDF shows has changed that may not: type size, line
/// spacing, margins, images, and how much of the text is left.
///
/// This reads the PDF, not the source, so it holds however the change was
/// made: a macro in another file, a class option, `\small` hidden in an
/// environment. Empty when nothing is off.
pub(crate) fn geometry_drift(base: &BodyMetrics, now: &BodyMetrics) -> Vec<String> {
    let mut out = Vec::new();
    if base.body_pages > 0 && now.body_pages > 0 {
        if now.font_size_pt < base.font_size_pt * FONT_TOLERANCE {
            out.push(format!(
                "the text is set smaller ({:.1} pt, was {:.1} pt)",
                now.font_size_pt, base.font_size_pt
            ));
        }
        if now.line_pitch_pt < base.line_pitch_pt * PITCH_TOLERANCE {
            out.push(format!(
                "the lines are set closer together ({:.1} pt apart, was {:.1} pt)",
                now.line_pitch_pt, base.line_pitch_pt
            ));
        }
        if now.text_width_pt > base.text_width_pt * BLOCK_TOLERANCE
            || now.text_height_pt > base.text_height_pt * BLOCK_TOLERANCE
        {
            out.push("the text block is larger, as if the margins were cut".to_owned());
        }
    }
    if now.images < base.images {
        out.push(format!(
            "{} image(s) are gone from the PDF",
            base.images - now.images
        ));
    }
    #[allow(clippy::cast_precision_loss)]
    if (now.chars as f32) < base.chars as f32 * KEEP_AT_LEAST {
        out.push(format!(
            "only {}% of the text is left",
            now.chars * 100 / base.chars.max(1)
        ));
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
        // Several short lines of one paragraph are recorded under the same
        // source line; the planner needs each paragraph once.
        let mut seen: Vec<(&str, u32)> = Vec::new();
        for p in &report.short_last_lines {
            if seen.len() == MAX_CANDIDATES {
                break;
            }
            if !seen.contains(&(p.file.as_str(), p.line)) {
                seen.push((p.file.as_str(), p.line));
                let _ = write!(out, "\n- {}", spot(p));
            }
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

    fn body() -> BodyMetrics {
        BodyMetrics {
            font_size_pt: 10.0,
            line_pitch_pt: 12.0,
            text_width_pt: 345.0,
            text_height_pt: 550.0,
            images: 4,
            chars: 30_000,
            body_pages: 6,
        }
    }

    #[test]
    fn what_the_pdf_shows_catches_every_way_of_cheating_whatever_the_source_says() {
        let base = body();
        assert_eq!(geometry_drift(&base, &base), Vec::<String>::new());

        // Wording cut by a fifth and a figure narrowed: allowed.
        let honest = BodyMetrics {
            chars: 24_000,
            ..base
        };
        assert_eq!(geometry_drift(&base, &honest), Vec::<String>::new());

        let drift = |m: BodyMetrics| geometry_drift(&base, &m).join("; ");
        assert!(
            drift(BodyMetrics {
                font_size_pt: 9.0,
                ..base
            })
            .contains("set smaller")
        );
        assert!(
            drift(BodyMetrics {
                line_pitch_pt: 11.0,
                ..base
            })
            .contains("closer together")
        );
        assert!(
            drift(BodyMetrics {
                text_height_pt: 580.0,
                ..base
            })
            .contains("margins")
        );
        assert!(
            drift(BodyMetrics {
                text_width_pt: 360.0,
                ..base
            })
            .contains("margins")
        );
        assert!(drift(BodyMetrics { images: 3, ..base }).contains("1 image(s) are gone"));
        assert!(
            drift(BodyMetrics {
                chars: 12_000,
                ..base
            })
            .contains("only 40% of the text")
        );
    }

    #[test]
    fn a_document_too_short_to_measure_is_judged_only_on_images_and_text() {
        let base = BodyMetrics {
            body_pages: 0,
            ..body()
        };
        let now = BodyMetrics {
            body_pages: 0,
            font_size_pt: 8.0,
            ..body()
        };
        assert_eq!(geometry_drift(&base, &now), Vec::<String>::new());
    }

    #[test]
    fn a_paragraph_recorded_on_several_short_lines_is_listed_once() {
        let text = feedback(
            &report(
                None,
                vec![place(4, 149), place(4, 149), place(4, 140), place(4, 149)],
            ),
            8,
        );
        assert_eq!(text.matches("main.tex:149").count(), 1, "{text}");
        assert_eq!(text.matches("main.tex:140").count(), 1, "{text}");
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
