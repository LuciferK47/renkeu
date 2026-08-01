//! `.synctex(.gz)` reader.
//!
//! The file is a line-oriented state machine:
//!
//! ```text
//! SyncTeX Version:1
//! Input:1:texput          <- tag -> name (repeats)
//! Magnification:1000
//! Unit:1
//! X Offset:0
//! Y Offset:0
//! Content:
//! {1                      <- open page 1
//! [1,7:4736287,46220575:26673152,41484288,0   <- vbox  tag,line:x,y:W,H,D
//! (1,7:8799519,8865055:22609920,454820,7208   <- hbox
//! h1,7:8799519,6571295:22609920,0,0           <- void hbox
//! g1,3:11475354,8865055                       <- glue  tag,line:x,y
//! k1,4:31409439,8865055:16503275              <- kern  tag,line:x,y:W
//! )                                           <- close hbox
//! ]                                           <- close vbox
//! }1                      <- close page
//! Postamble:
//! ```
//!
//! Some engines emit `tag,line,column:`; `xetex` (what Tectonic uses) emits
//! only `tag,line:`. Both are accepted.
//!
//! We flatten the box tree: reverse search only needs the tightest node
//! containing a point, which a flat list with extents answers directly.

use crate::{Node, NodeKind, SyncTexData, SyncTexError};
use flate2::read::GzDecoder;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Read and parse a `SyncTeX` file, transparently gunzipping a `.gz` path.
///
/// # Errors
/// [`SyncTexError::Io`] if the file cannot be read or decompressed.
pub fn parse_file(path: &Path) -> Result<SyncTexData, SyncTexError> {
    let mut file = File::open(path)?;
    let mut content = String::new();

    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        GzDecoder::new(file).read_to_string(&mut content)?;
    } else {
        file.read_to_string(&mut content)?;
    }

    Ok(parse_content(&content))
}

/// Parse from gzipped bytes (the shape the compiler hands back in-memory).
///
/// # Errors
/// [`SyncTexError::Io`] if the gzip stream is invalid.
pub fn parse_gz_bytes(bytes: &[u8]) -> Result<SyncTexData, SyncTexError> {
    let mut content = String::new();
    GzDecoder::new(bytes).read_to_string(&mut content)?;
    Ok(parse_content(&content))
}

/// Parse an already-decompressed document. Malformed records are skipped
/// rather than failing the whole parse — a partial map beats none.
#[must_use]
pub fn parse_content(content: &str) -> SyncTexData {
    let mut data = SyncTexData {
        version: 1,
        magnification: 1000,
        unit: 1,
        x_offset: 0,
        y_offset: 0,
        inputs: HashMap::new(),
        pages: BTreeMap::new(),
    };

    let mut lines = content.lines();

    // --- preamble ---
    for line in lines.by_ref() {
        if line == "Content:" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Input:") {
            if let Some((tag, name)) = rest.split_once(':') {
                if let Ok(tag) = tag.parse::<u32>() {
                    data.inputs.insert(tag, name.to_owned());
                }
            }
        } else if let Some(v) = line.strip_prefix("SyncTeX Version:") {
            data.version = v.trim().parse().unwrap_or(1);
        } else if let Some(v) = line.strip_prefix("Magnification:") {
            data.magnification = v.trim().parse().unwrap_or(1000);
        } else if let Some(v) = line.strip_prefix("Unit:") {
            data.unit = v.trim().parse().unwrap_or(1);
        } else if let Some(v) = line.strip_prefix("X Offset:") {
            data.x_offset = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Y Offset:") {
            data.y_offset = v.trim().parse().unwrap_or(0);
        }
    }

    // --- content ---
    let (unit, x_offset, y_offset) = (data.unit, data.x_offset, data.y_offset);
    let mut current_page: Option<(u32, Vec<Node>)> = None;

    for line in lines {
        if line.starts_with("Postamble:") {
            break;
        }
        if let Some(page_digits) = line.strip_prefix('{') {
            // Close any page left open by a malformed file.
            if let Some((num, nodes)) = current_page.take() {
                data.pages.insert(num, nodes);
            }
            let num = page_digits.trim().parse::<u32>().unwrap_or(1);
            current_page = Some((num, Vec::new()));
            continue;
        }
        if line.starts_with('}') {
            if let Some((num, nodes)) = current_page.take() {
                data.pages.insert(num, nodes);
            }
            continue;
        }

        let Some((_, nodes)) = current_page.as_mut() else {
            continue; // stray record outside a page
        };
        if let Some(node) = parse_record(line, unit, x_offset, y_offset) {
            nodes.push(node);
        }
    }

    if let Some((num, nodes)) = current_page.take() {
        data.pages.insert(num, nodes);
    }

    data
}

/// Parse one content record. Returns `None` for closers (`)`/`]`), hints
/// (`!42`), blank lines, and anything malformed.
fn parse_record(line: &str, unit: i64, x_offset: i64, y_offset: i64) -> Option<Node> {
    let prefix = line.chars().next()?;
    let kind = match prefix {
        '[' => NodeKind::VBox,
        '(' => NodeKind::HBox,
        'v' => NodeKind::VoidVBox,
        'h' => NodeKind::VoidHBox,
        'g' => NodeKind::Glue,
        'k' => NodeKind::Kern,
        '$' => NodeKind::Math,
        'x' => NodeKind::Current,
        _ => return None, // ')', ']', '!', '<', '>', blank …
    };

    // `tag,line[,column]:x,y[:W,H,D | :W]`
    let mut fields = line.get(1..)?.split(':');

    let (tag, source_line, column) = parse_location(fields.next()?)?;
    let (raw_x, raw_y) = parse_pair(fields.next()?)?;

    let (width, height, depth) = fields.next().map_or((0, 0, 0), parse_extent);

    Some(Node {
        tag,
        line: source_line,
        column,
        kind,
        x: raw_x * unit + x_offset,
        y: raw_y * unit + y_offset,
        width: width * unit,
        height: height * unit,
        depth: depth * unit,
    })
}

/// `tag,line` or `tag,line,column`.
fn parse_location(field: &str) -> Option<(u32, u32, Option<u32>)> {
    let mut parts = field.split(',');
    let tag = parts.next()?.trim().parse().ok()?;
    let line = parts.next()?.trim().parse().ok()?;
    let column = parts.next().and_then(|c| c.trim().parse().ok());
    Some((tag, line, column))
}

/// `x,y`.
fn parse_pair(field: &str) -> Option<(i64, i64)> {
    let (a, b) = field.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// `W`, `W,H`, or `W,H,D`; missing components are 0.
fn parse_extent(field: &str) -> (i64, i64, i64) {
    let mut parts = field.split(',');
    let w = parts
        .next()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let h = parts
        .next()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let d = parts
        .next()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    (w, h, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from real Tectonic output (see the module docs).
    const SAMPLE: &str = "SyncTeX Version:1\n\
        Input:1:texput\n\
        Input:2:\n\
        Output:pdf\n\
        Magnification:1000\n\
        Unit:1\n\
        X Offset:0\n\
        Y Offset:0\n\
        Content:\n\
        !146\n\
        {1\n\
        [1,7:4736287,46220575:26673152,41484288,0\n\
        (1,4:8799519,8865055:22609920,454820,7208\n\
        h1,3:8799519,8865055:983040,0,0\n\
        g1,3:11475354,8865055\n\
        k1,4:31409439,8865055:16503275\n\
        )\n\
        ]\n\
        }1\n\
        {2\n\
        [1,9:4736287,46220575:26673152,41484288,0\n\
        }2\n\
        Postamble:\n";

    #[test]
    fn reads_preamble() {
        let d = parse_content(SAMPLE);
        assert_eq!(d.version, 1);
        assert_eq!(d.magnification, 1000);
        assert_eq!(d.unit, 1);
        assert_eq!(d.inputs.get(&1).map(String::as_str), Some("texput"));
        assert_eq!(
            d.primary_tag(),
            Some(1),
            "tag 2 is empty and must be skipped"
        );
    }

    #[test]
    fn splits_pages_and_skips_non_records() {
        let d = parse_content(SAMPLE);
        assert_eq!(d.pages.len(), 2);
        // `!146`, `)` and `]` are not nodes.
        assert_eq!(d.pages[&1].len(), 5);
        assert_eq!(d.pages[&2].len(), 1);
    }

    #[test]
    fn parses_box_geometry() {
        let d = parse_content(SAMPLE);
        let vbox = &d.pages[&1][0];
        assert_eq!(vbox.kind, NodeKind::VBox);
        assert_eq!((vbox.tag, vbox.line), (1, 7));
        assert_eq!((vbox.x, vbox.y), (4_736_287, 46_220_575));
        assert_eq!(
            (vbox.width, vbox.height, vbox.depth),
            (26_673_152, 41_484_288, 0)
        );
        // Reference point is the baseline: box spans y-height ..= y+depth.
        assert_eq!(
            vbox.bounds(),
            (
                4_736_287,
                46_220_575 - 41_484_288,
                4_736_287 + 26_673_152,
                46_220_575
            )
        );
    }

    #[test]
    fn parses_dimensionless_and_kern_records() {
        let d = parse_content(SAMPLE);
        let glue = d.pages[&1]
            .iter()
            .find(|n| n.kind == NodeKind::Glue)
            .unwrap();
        assert_eq!((glue.width, glue.height, glue.depth), (0, 0, 0));
        assert_eq!((glue.x, glue.y), (11_475_354, 8_865_055));

        let kern = d.pages[&1]
            .iter()
            .find(|n| n.kind == NodeKind::Kern)
            .unwrap();
        assert_eq!(kern.width, 16_503_275, "kern carries only a width");
        assert_eq!(kern.height, 0);
    }

    #[test]
    fn accepts_an_optional_column_field() {
        // Engines other than xetex emit tag,line,column.
        let node = parse_record("(1,4,17:100,200:300,40,5", 1, 0, 0).unwrap();
        assert_eq!((node.tag, node.line, node.column), (1, 4, Some(17)));
        assert_eq!((node.x, node.y), (100, 200));
        assert_eq!((node.width, node.height, node.depth), (300, 40, 5));
    }

    #[test]
    fn applies_unit_and_offsets() {
        let node = parse_record("(1,4:100,200:300,40,5", 2, 10, 20).unwrap();
        assert_eq!((node.x, node.y), (100 * 2 + 10, 200 * 2 + 20));
        assert_eq!((node.width, node.height), (600, 80));
    }

    #[test]
    fn rejects_closers_hints_and_malformed_records() {
        for line in [")", "]", "!146", "", "(1", "(notanumber,4:1,2"] {
            assert!(
                parse_record(line, 1, 0, 0).is_none(),
                "should skip: {line:?}"
            );
        }
    }

    #[test]
    fn one_inch_margin_is_seventy_two_points() {
        // Anchors the sp->bp factor against a known-good value from real
        // output: the default 1in article margin is 4736287 sp.
        let d = parse_content(SAMPLE);
        let x_bp = d.sp_to_bp(4_736_287);
        assert!((x_bp - 72.0).abs() < 0.01, "expected 72bp, got {x_bp}");
        assert!(
            (d.bp_to_sp(72.0) - 4_736_287).abs() <= 1,
            "round-trips within 1sp"
        );
    }
}
