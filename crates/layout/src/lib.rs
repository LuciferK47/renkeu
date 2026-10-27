//! Facts about a compiled document's layout, with no AI involved: how many
//! pages it has, how full the last one is, where it crosses a page budget,
//! which boxes the log calls overfull or underfull, and which paragraphs end
//! in a very short line (the cheapest place to win back space).
//!
//! Everything is read from what the compile already produced: the `SyncTeX`
//! map says where each line of the source landed, the log says which boxes
//! TeX complained about. A chatbot sees neither; this is the part of "fit it
//! to N pages" that does not need a model.
//!
//! The line analysis is a heuristic over boxes, not a typesetting engine:
//! "line" means a horizontal box inside a vertical one, which also matches
//! table rows and the like. It reports candidates, and says so.
//!
//! Two facts about the map make it work. TeX makes every line of a paragraph
//! as wide as the column, so a line's *box* says nothing about how much of it
//! is text; the glue and kerns inside it do (the last of them marks where the
//! text ends, before the stretchy fill that closes a paragraph). And the map
//! records each line of a paragraph under the paragraph's *last* source line,
//! which also tells lines of one paragraph from their neighbours.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use serde::Serialize;
use std::collections::HashMap;
use synctex::{NodeKind, SyncTexData};
use texlog::{Diagnostic, DiagnosticKind};

/// Most boxes and lines listed; the totals are still counted.
const MAX_LISTED: usize = 100;
/// One TeX point in scaled points.
const PT: i64 = 65_536;
/// A line is a candidate "short last line" below this share of the column.
const SHORT_LINE_SHARE: f64 = 0.25;

/// How the caller maps between the map's input tags and project files.
pub struct Resolver<'a> {
    /// Project-relative file for an input tag.
    pub file_of: &'a dyn Fn(u32) -> String,
    /// Input tag for a project-relative file, if the map has one.
    pub tag_of: &'a dyn Fn(&str) -> Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The page budget, if the document has one.
    pub page_limit: Option<u32>,
}

/// Where something is: the page, the PDF region in points from the page's
/// top-left corner, and the source line behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fill {
    /// Height the text on the last page occupies, in points.
    pub last_page_pt: f64,
    /// The same for the fullest page.
    pub fullest_page_pt: f64,
    /// `last / fullest`, or `None` for a one-page document (nothing to compare).
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Budget {
    pub limit: u32,
    pub pages: u32,
    pub over: bool,
    /// Lines of text on the pages past the limit.
    pub lines_over: usize,
    /// The first line that no longer fits, when `over`.
    pub first_over: Option<Place>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoxIssue {
    pub overfull: bool,
    pub horizontal: bool,
    /// How far it is off, in points, when the log says.
    pub amount_pt: Option<f64>,
    pub message: String,
    pub place: Option<Place>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutReport {
    pub pages: u32,
    pub fill: Fill,
    pub budget: Option<Budget>,
    pub boxes: Vec<BoxIssue>,
    pub boxes_total: usize,
    pub short_last_lines: Vec<Place>,
    pub short_last_lines_total: usize,
}

/// One line of text as the map records it.
#[derive(Debug, Clone)]
struct Line {
    page: u32,
    tag: u32,
    /// The source line the map records it under.
    src: u32,
    /// sp, y growing downward.
    left: i64,
    top: i64,
    width: i64,
    bottom: i64,
    parent: usize,
    /// How far the text in the line reaches from its left edge (sp); the
    /// whole width when the line has nothing to measure by.
    used: i64,
}

/// The lines of text on every page. Running heads and feet (the page number)
/// are not text: they sit on the very top and bottom edge of the page's box,
/// where the body never does, so lines touching those edges are left out.
fn lines(data: &SyncTexData) -> Vec<Line> {
    let mut out = Vec::new();
    for (&page, nodes) in &data.pages {
        let (page_top, page_bottom) = nodes
            .iter()
            .find(|n| n.parent.is_none() && n.kind == NodeKind::VBox)
            .map_or((i64::MIN, i64::MAX), |root| {
                let (_, top, _, bottom) = root.bounds();
                (top + PT / 2, bottom - PT / 2)
            });
        let mut children: HashMap<usize, Vec<&synctex::Node>> = HashMap::new();
        for node in nodes {
            if let Some(parent) = node.parent {
                children.entry(parent).or_default().push(node);
            }
        }
        for (index, node) in nodes.iter().enumerate() {
            let Some(parent) = node.parent else { continue };
            let inside_vbox = nodes
                .get(parent)
                .is_some_and(|p| matches!(p.kind, NodeKind::VBox));
            if node.kind == NodeKind::HBox && inside_vbox && node.width > 0 {
                let (left, top, right, bottom) = node.bounds();
                // Glue and kerns sitting at the right edge are the fill that
                // ends the line; what is left of that is where the text stops.
                let edge = right - PT;
                let used = children
                    .get(&index)
                    .into_iter()
                    .flatten()
                    .filter(|c| c.x < edge)
                    .map(|c| (c.x + c.width.max(0)).min(right) - left)
                    .max()
                    .unwrap_or(node.width);
                let furniture = top <= page_top || bottom >= page_bottom;
                if bottom > top && !furniture {
                    out.push(Line {
                        page,
                        tag: node.tag,
                        src: node.line,
                        left,
                        top,
                        width: node.width,
                        bottom,
                        parent,
                        used,
                    });
                }
            }
        }
    }
    out
}

fn place(data: &SyncTexData, resolver: &Resolver, l: &Line) -> Place {
    Place {
        page: l.page,
        x: data.sp_to_bp(l.left),
        y: data.sp_to_bp(l.top),
        width: data.sp_to_bp(l.width),
        height: data.sp_to_bp(l.bottom - l.top),
        file: (resolver.file_of)(l.tag),
        line: l.src,
    }
}

/// The most common line width: the column (rounded to a point, so kerning
/// noise does not split the vote).
fn column_width(lines: &[Line]) -> Option<i64> {
    let mut votes: HashMap<i64, usize> = HashMap::new();
    for l in lines {
        *votes.entry(l.width / 65_536).or_default() += 1;
    }
    let (rounded, _) = votes.into_iter().max_by_key(|&(w, n)| (n, w))?;
    lines
        .iter()
        .map(|l| l.width)
        .filter(|w| w / 65_536 == rounded)
        .max()
}

/// The amount in "(12.3pt too wide)" or "(5pt too high)".
fn parse_amount(message: &str) -> Option<f64> {
    let start = message.find('(')? + 1;
    let rest = &message[start..];
    let end = rest.find("pt")?;
    rest[..end].trim().parse().ok()
}

fn box_issues(
    data: &SyncTexData,
    diagnostics: &[Diagnostic],
    resolver: &Resolver,
) -> Vec<BoxIssue> {
    diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.kind,
                DiagnosticKind::OverfullBox | DiagnosticKind::UnderfullBox
            )
        })
        .map(|d| {
            let place = d.file.as_deref().zip(d.line).and_then(|(file, line)| {
                let tag = (resolver.tag_of)(file)?;
                let hit = data.forward_search(tag, line).into_iter().next()?;
                Some(Place {
                    page: hit.page,
                    x: hit.x,
                    y: hit.y,
                    width: hit.width,
                    height: hit.height,
                    file: file.to_owned(),
                    line: hit.line,
                })
            });
            BoxIssue {
                overfull: d.kind == DiagnosticKind::OverfullBox,
                horizontal: d.message.contains("\\hbox"),
                amount_pt: parse_amount(&d.message),
                message: d.message.clone(),
                place,
            }
        })
        .collect()
}

/// Read the layout facts for one compile.
#[must_use]
pub fn analyze(
    data: &SyncTexData,
    diagnostics: &[Diagnostic],
    resolver: &Resolver,
    options: &Options,
) -> LayoutReport {
    let pages = data.pages.keys().next_back().copied().unwrap_or(0);
    let all = lines(data);

    // How much of each page the text uses.
    let mut extent: HashMap<u32, (i64, i64)> = HashMap::new();
    for l in &all {
        let e = extent.entry(l.page).or_insert((l.top, l.bottom));
        e.0 = e.0.min(l.top);
        e.1 = e.1.max(l.bottom);
    }
    let used = |page: u32| {
        extent
            .get(&page)
            .map_or(0.0, |&(t, b)| data.sp_to_bp(b - t))
    };
    let fullest = (1..=pages).map(used).fold(0.0_f64, f64::max);
    let last = used(pages);
    let fill = Fill {
        last_page_pt: last,
        fullest_page_pt: fullest,
        ratio: (pages > 1 && fullest > 0.0).then(|| last / fullest),
    };

    let budget = options.page_limit.map(|limit| {
        let over = pages > limit;
        let beyond: Vec<&Line> = all.iter().filter(|l| l.page > limit).collect();
        let first_over = over
            .then(|| {
                beyond
                    .iter()
                    .filter(|l| l.page == limit + 1)
                    .min_by_key(|l| l.top)
                    .map(|l| place(data, resolver, l))
            })
            .flatten();
        Budget {
            limit,
            pages,
            over,
            lines_over: beyond.len(),
            first_over,
        }
    });

    // The last line of a paragraph is a candidate when the text in it stops
    // within the first quarter of the column: a word or two that spilled over.
    let mut short = Vec::new();
    if let Some(column) = column_width(&all) {
        let mut previous: HashMap<(u32, usize), &Line> = HashMap::new();
        for l in &all {
            if let Some(above) = previous.get(&(l.page, l.parent)) {
                let same_paragraph = above.tag == l.tag && above.src == l.src;
                let height = (above.bottom - above.top).max(1);
                let close = l.top - above.top < height * 5 / 2;
                #[allow(clippy::cast_precision_loss)]
                let narrow = (l.used as f64) < column as f64 * SHORT_LINE_SHARE;
                if same_paragraph && close && narrow {
                    short.push(place(data, resolver, l));
                }
            }
            previous.insert((l.page, l.parent), l);
        }
    }

    let mut boxes = box_issues(data, diagnostics, resolver);
    let boxes_total = boxes.len();
    boxes.truncate(MAX_LISTED);
    let short_last_lines_total = short.len();
    short.truncate(MAX_LISTED);

    LayoutReport {
        pages,
        fill,
        budget,
        boxes,
        boxes_total,
        short_last_lines: short,
        short_last_lines_total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use synctex::Node;
    use texlog::Severity;

    const PT: i64 = 65_536;

    fn node(
        line: u32,
        kind: NodeKind,
        x: i64,
        y: i64,
        w: i64,
        h: i64,
        parent: Option<usize>,
    ) -> Node {
        Node {
            tag: 1,
            line,
            column: None,
            kind,
            x: x * PT,
            y: y * PT,
            width: w * PT,
            height: h * PT,
            depth: 0,
            parent,
        }
    }

    /// A page: its box, a column box, then `(line, y, used)` lines: boxes as
    /// wide as the column whose text reaches `used` points in, ended by the
    /// fill glue at the right edge, as TeX leaves them.
    fn page(lines: &[(u32, i64, i64)]) -> Vec<Node> {
        let mut nodes = vec![
            // The page box runs from y = 72 down to y = 700.
            node(0, NodeKind::VBox, 72, 700, 400, 628, None),
            node(0, NodeKind::VBox, 72, 700, 400, 628, Some(0)),
        ];
        for &(line, y, used) in lines {
            let parent = nodes.len();
            nodes.push(node(line, NodeKind::HBox, 72, y, 400, 10, Some(1)));
            if used < 400 {
                nodes.push(node(line, NodeKind::Glue, 72 + used, y, 0, 0, Some(parent)));
            }
            // The paragraph's closing fill sits at the right edge.
            nodes.push(node(line, NodeKind::Glue, 72 + 400, y, 0, 0, Some(parent)));
        }
        nodes
    }

    fn data(pages: Vec<Vec<Node>>) -> SyncTexData {
        SyncTexData {
            version: 1,
            magnification: 1000,
            unit: 1,
            x_offset: 0,
            y_offset: 0,
            inputs: HashMap::from([(1, "main.tex".to_owned())]),
            pages: pages
                .into_iter()
                .enumerate()
                .map(|(i, n)| (u32::try_from(i + 1).unwrap(), n))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn report(d: &SyncTexData, diags: &[Diagnostic], limit: Option<u32>) -> LayoutReport {
        let file_of = |_: u32| "main.tex".to_owned();
        let tag_of = |f: &str| (f == "main.tex").then_some(1);
        analyze(
            d,
            diags,
            &Resolver {
                file_of: &file_of,
                tag_of: &tag_of,
            },
            &Options { page_limit: limit },
        )
    }

    fn full(count: i64, first_line: u32) -> Vec<(u32, i64, i64)> {
        (0..count)
            .map(|i| (first_line + u32::try_from(i).unwrap(), 100 + i * 14, 400))
            .collect()
    }

    #[test]
    fn counts_pages_and_compares_the_last_with_the_fullest() {
        let d = data(vec![page(&full(40, 1)), page(&full(10, 41))]);
        let r = report(&d, &[], None);
        assert_eq!(r.pages, 2);
        assert!(r.fill.fullest_page_pt > r.fill.last_page_pt);
        let ratio = r.fill.ratio.unwrap();
        assert!((0.2..0.3).contains(&ratio), "{ratio}");
        assert!(
            report(&data(vec![page(&full(5, 1))]), &[], None)
                .fill
                .ratio
                .is_none()
        );
    }

    #[test]
    fn the_page_number_in_the_foot_is_not_text() {
        // Each page has one text line and a page number on the bottom edge.
        let with_foot = |line: u32| {
            let mut nodes = page(&[(line, 100, 400)]);
            let at = nodes.len();
            nodes.push(node(line, NodeKind::HBox, 72, 700, 400, 6, Some(1)));
            nodes.push(node(line, NodeKind::Glue, 72 + 200, 700, 0, 0, Some(at)));
            nodes
        };
        let d = data(vec![with_foot(1), with_foot(2)]);
        let r = report(&d, &[], Some(1));
        assert_eq!(
            r.budget.unwrap().lines_over,
            1,
            "the foot of page 2 must not count"
        );
        assert!(r.fill.fullest_page_pt < 20.0, "{:?}", r.fill);
    }

    #[test]
    fn an_empty_map_is_an_empty_report() {
        let r = report(&data(vec![]), &[], Some(8));
        assert_eq!(r.pages, 0);
        assert!(!r.budget.unwrap().over);
        assert!(r.boxes.is_empty() && r.short_last_lines.is_empty());
    }

    #[test]
    fn a_budget_names_the_first_line_that_no_longer_fits() {
        let d = data(vec![
            page(&full(40, 1)),
            page(&full(30, 41)),
            page(&full(6, 71)),
        ]);
        let b = report(&d, &[], Some(2)).budget.unwrap();
        assert!(b.over);
        assert_eq!((b.limit, b.pages, b.lines_over), (2, 3, 6));
        let first = b.first_over.unwrap();
        assert_eq!(
            (first.page, first.line, first.file.as_str()),
            (3, 71, "main.tex")
        );

        let within = report(&d, &[], Some(3)).budget.unwrap();
        assert!(!within.over && within.first_over.is_none() && within.lines_over == 0);
    }

    #[test]
    fn the_end_of_a_paragraph_with_a_few_words_is_a_candidate_but_a_heading_is_not() {
        // Four lines of one paragraph (all recorded under its last line, 4),
        // the last with text only 40 points in; then a heading alone.
        let lines = [
            (4, 100, 400),
            (4, 114, 400),
            (4, 128, 400),
            (4, 142, 40),
            (9, 100 + 8 * 14, 40),
        ];
        let r = report(&data(vec![page(&lines)]), &[], None);
        assert_eq!(r.short_last_lines_total, 1);
        assert_eq!(r.short_last_lines[0].line, 4);
        assert_eq!(r.short_last_lines[0].page, 1);
    }

    #[test]
    fn a_last_line_that_is_most_of_a_line_is_not_a_candidate() {
        let lines = [(4, 100, 400), (4, 114, 300)];
        assert_eq!(
            report(&data(vec![page(&lines)]), &[], None).short_last_lines_total,
            0
        );
    }

    #[test]
    fn a_short_line_that_belongs_to_the_next_paragraph_is_not_the_last_line_of_this_one() {
        // Same wording, but the short line was recorded under another line.
        let lines = [(4, 100, 400), (5, 114, 40)];
        assert_eq!(
            report(&data(vec![page(&lines)]), &[], None).short_last_lines_total,
            0
        );
    }

    #[test]
    fn a_short_line_far_below_is_not_a_continuation() {
        let lines = [(4, 100, 400), (4, 400, 40)];
        assert_eq!(
            report(&data(vec![page(&lines)]), &[], None).short_last_lines_total,
            0
        );
    }

    fn overfull(line: u32, message: &str) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            kind: DiagnosticKind::OverfullBox,
            file: Some("main.tex".to_owned()),
            line: Some(line),
            message: message.to_owned(),
            context: None,
        }
    }

    #[test]
    fn overfull_boxes_get_their_size_and_their_place_on_the_page() {
        let d = data(vec![page(&full(5, 1))]);
        let r = report(
            &d,
            &[overfull(
                3,
                "Overfull \\hbox (12.5pt too wide) in paragraph at lines 3--3",
            )],
            None,
        );
        assert_eq!(r.boxes_total, 1);
        let b = &r.boxes[0];
        assert!(b.overfull && b.horizontal);
        assert_eq!(b.amount_pt, Some(12.5));
        let p = b.place.as_ref().unwrap();
        assert_eq!((p.page, p.line), (1, 3));
    }

    #[test]
    fn a_box_in_a_file_the_map_does_not_know_has_no_place() {
        let mut diag = overfull(3, "Overfull \\vbox (3pt too high)");
        diag.file = Some("other.tex".to_owned());
        let r = report(&data(vec![page(&full(5, 1))]), &[diag], None);
        assert!(r.boxes[0].place.is_none());
        assert!(!r.boxes[0].horizontal);
    }

    #[test]
    fn the_amount_is_read_from_the_log_wording() {
        assert_eq!(parse_amount("Overfull \\hbox (0.5pt too wide)"), Some(0.5));
        assert_eq!(
            parse_amount("Underfull \\hbox (badness 10000) in paragraph"),
            None
        );
    }
}
