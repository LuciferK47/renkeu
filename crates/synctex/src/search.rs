//! Reverse (PDF -> source) and forward (source -> PDF) lookup.

use crate::{Node, SyncTexData};

/// A source position recovered from a PDF coordinate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHit {
    /// Input tag; resolve through [`SyncTexData::inputs`].
    pub tag: u32,
    /// Recorded input name for `tag` (Tectonic records `texput` for the
    /// primary input, so callers usually substitute their own path).
    pub file: String,
    /// 1-based source line.
    pub line: u32,
    pub column: Option<u32>,
    /// `true` when the point fell inside the matched box rather than merely
    /// near it — lets callers judge how much to trust the hit.
    pub exact: bool,
}

/// A PDF region produced by a source line.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfHit {
    pub page: u32,
    /// Box bounds in PDF points from the page's top-left corner.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The source line that actually matched (may be after the requested one).
    pub line: u32,
}

/// Nearest source position to a point on `page`, given in PDF points from the
/// page's top-left corner.
///
/// Prefers the *tightest* box containing the point — nesting means the outer
/// page vbox contains everything, so picking by smallest area is what makes
/// the answer specific. Falls back to the nearest node when nothing contains
/// the point (e.g. a click in a margin).
#[must_use]
pub fn backward_search(data: &SyncTexData, page: u32, x_bp: f64, y_bp: f64) -> Option<SourceHit> {
    let nodes = data.pages.get(&page)?;
    let x = data.bp_to_sp(x_bp);
    let y = data.bp_to_sp(y_bp);

    // Records with tag 0 or line 0 carry no usable source position.
    let usable = || nodes.iter().filter(|n| n.tag != 0 && n.line != 0);

    let containing = usable()
        .filter(|n| n.contains(x, y))
        // Tightest wins; among equals prefer a real box over a point node.
        .min_by(|a, b| {
            a.area()
                .cmp(&b.area())
                .then_with(|| b.kind.is_box().cmp(&a.kind.is_box()))
        });

    let node = match containing {
        Some(n) => Some((n, true)),
        None => usable()
            .min_by(|a, b| a.distance_sq(x, y).cmp(&b.distance_sq(x, y)))
            .map(|n| (n, false)),
    };

    node.map(|(n, exact)| SourceHit {
        tag: n.tag,
        file: data.inputs.get(&n.tag).cloned().unwrap_or_default(),
        line: n.line,
        column: n.column,
        exact,
    })
}

/// PDF regions produced by `line` of input `tag`, best match first.
///
/// Exact line matches come first (largest box first, so the caller can
/// highlight the whole line); if the line produced no output — a comment, a
/// blank line — the nearest following line that did is used instead, which is
/// what a "jump to source position" affordance wants.
#[must_use]
pub fn forward_search(data: &SyncTexData, tag: u32, line: u32) -> Vec<PdfHit> {
    let mut exact = Vec::new();
    let mut best_following: Option<u32> = None;

    for (page, nodes) in &data.pages {
        for node in nodes.iter().filter(|n| n.tag == tag) {
            if node.line == line {
                exact.push(to_hit(data, *page, node));
            } else if node.line > line {
                best_following = Some(best_following.map_or(node.line, |b| b.min(node.line)));
            }
        }
    }

    if !exact.is_empty() {
        exact.sort_by(|a, b| {
            (b.width * b.height)
                .partial_cmp(&(a.width * a.height))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.page.cmp(&b.page))
        });
        return exact;
    }

    let Some(fallback_line) = best_following else {
        return Vec::new();
    };

    let mut hits: Vec<PdfHit> = data
        .pages
        .iter()
        .flat_map(|(page, nodes)| {
            nodes
                .iter()
                .filter(|n| n.tag == tag && n.line == fallback_line)
                .map(move |n| to_hit(data, *page, n))
        })
        .collect();
    hits.sort_by_key(|h| h.page);
    hits
}

fn to_hit(data: &SyncTexData, page: u32, node: &Node) -> PdfHit {
    let (left, top, right, bottom) = node.bounds();
    PdfHit {
        page,
        x: data.sp_to_bp(left),
        y: data.sp_to_bp(top),
        width: data.sp_to_bp(right - left),
        height: data.sp_to_bp(bottom - top),
        line: node.line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_content;

    /// Two stacked lines inside one page vbox, plus a second page.
    /// Line 10's box spans y 100..200bp, line 20's spans y 300..400bp.
    fn fixture() -> SyncTexData {
        #[allow(clippy::cast_possible_truncation)]
        let bp = |v: f64| (v * crate::SP_PER_BP).round() as i64;
        let doc = format!(
            "SyncTeX Version:1\n\
             Input:1:texput\n\
             Magnification:1000\n\
             Unit:1\n\
             X Offset:0\n\
             Y Offset:0\n\
             Content:\n\
             {{1\n\
             [1,5:{outer_x},{outer_y}:{outer_w},{outer_h},0\n\
             (1,10:{ax},{ay}:{aw},{ah},0\n\
             (1,20:{bx},{by}:{bw},{bh},0\n\
             ]\n\
             }}1\n\
             {{2\n\
             (1,50:{ax},{ay}:{aw},{ah},0\n\
             }}2\n\
             Postamble:\n",
            // outer box covers the whole text area: y 50..500bp
            outer_x = bp(50.0),
            outer_y = bp(500.0),
            outer_w = bp(500.0),
            outer_h = bp(450.0),
            // line 10: x 100..300, y 100..200 (baseline 200, height 100)
            ax = bp(100.0),
            ay = bp(200.0),
            aw = bp(200.0),
            ah = bp(100.0),
            // line 20: x 100..300, y 300..400
            bx = bp(100.0),
            by = bp(400.0),
            bw = bp(200.0),
            bh = bp(100.0),
        );
        parse_content(&doc)
    }

    #[test]
    fn point_inside_a_line_box_resolves_to_that_line() {
        let d = fixture();
        let hit = backward_search(&d, 1, 150.0, 150.0).expect("hit");
        assert_eq!(hit.line, 10);
        assert!(hit.exact);
        assert_eq!(hit.tag, 1);
    }

    #[test]
    fn tightest_enclosing_box_wins_over_the_page_vbox() {
        let d = fixture();
        // Inside both the outer vbox and line 20's hbox.
        let hit = backward_search(&d, 1, 200.0, 350.0).expect("hit");
        assert_eq!(hit.line, 20, "must not fall back to the enclosing vbox");
    }

    #[test]
    fn point_outside_every_box_falls_back_to_the_nearest() {
        let d = fixture();
        // Page 2 holds a single hbox (line 50, x 100..300, y 100..200) and no
        // enclosing vbox, so the fallback is unambiguous here.
        let hit = backward_search(&d, 2, 500.0, 150.0).expect("hit");
        assert!(!hit.exact, "should be reported as inexact");
        assert_eq!(hit.line, 50);
    }

    #[test]
    fn whitespace_inside_the_text_block_resolves_to_the_enclosing_box() {
        let d = fixture();
        // Between the two lines: no line box contains it, but the page vbox
        // does. Documented limitation — the answer is the block, not a line.
        let hit = backward_search(&d, 1, 200.0, 250.0).expect("hit");
        assert!(hit.exact);
        assert_eq!(hit.line, 5, "the enclosing vbox's line");
    }

    #[test]
    fn unknown_page_returns_none() {
        assert!(backward_search(&fixture(), 99, 100.0, 100.0).is_none());
    }

    #[test]
    fn searches_are_page_scoped() {
        let d = fixture();
        let hit = backward_search(&d, 2, 150.0, 150.0).expect("hit");
        assert_eq!(hit.line, 50, "page 2 has only line 50");
    }

    #[test]
    fn forward_search_returns_the_line_rect_in_points() {
        let d = fixture();
        let hits = forward_search(&d, 1, 10);
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert_eq!(h.page, 1);
        assert!((h.x - 100.0).abs() < 0.5, "x was {}", h.x);
        assert!((h.y - 100.0).abs() < 0.5, "y (top) was {}", h.y);
        assert!((h.width - 200.0).abs() < 0.5, "width was {}", h.width);
        assert!((h.height - 100.0).abs() < 0.5, "height was {}", h.height);
    }

    #[test]
    fn forward_search_falls_forward_when_a_line_produced_no_output() {
        let d = fixture();
        // Line 15 typesets nothing; the next line that does is 20.
        let hits = forward_search(&d, 1, 15);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 20);
    }

    #[test]
    fn forward_search_past_the_end_returns_nothing() {
        assert_eq!(forward_search(&fixture(), 1, 9999).len(), 0);
    }

    #[test]
    fn round_trips_backward_then_forward() {
        let d = fixture();
        let hit = backward_search(&d, 1, 150.0, 150.0).expect("backward hit");
        let back = forward_search(&d, hit.tag, hit.line);
        assert_ne!(back.len(), 0);
        assert_eq!(back[0].page, 1);
        // The rect we get back must contain the point we started from.
        let h = &back[0];
        assert!(h.x <= 150.0 && 150.0 <= h.x + h.width);
        assert!(h.y <= 150.0 && 150.0 <= h.y + h.height);
    }
}
