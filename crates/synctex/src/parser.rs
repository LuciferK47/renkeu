//! `.synctex` / `.synctex.gz` reader.
//!
//! The file is a line-oriented state machine: a preamble of `Input:<id>:<path>`
//! records, a `Content:` marker, then `{<page>` … `}` blocks holding one record
//! per line. We flatten each page's records into a list because
//! [`crate::search::backward_search`] only needs nearest-box lookup, not the
//! nesting.
//!
//! Note (M3): record fields are read positionally as
//! `<prefix>link,line:col:x,y:W,H,D`. Widening this to the full `SyncTeX` grammar
//! — and the scaled-point/pixel unit conversion `backward_search` still lacks —
//! is tracked as M3 work (masterplan.md §12 M3), not done here.

use crate::{BoxData, NodeData, Page, SyncTexData, SyncTexError, SyncTexNode};
use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Read and parse a `SyncTeX` file, transparently gunzipping a `.gz` path.
///
/// # Errors
/// Returns [`SyncTexError::Io`] if the file cannot be read or decompressed.
pub fn parse_file(path: &Path) -> Result<SyncTexData, SyncTexError> {
    let mut file = File::open(path)?;
    let mut content = String::new();

    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        let mut gz = GzDecoder::new(file);
        gz.read_to_string(&mut content)?;
    } else {
        file.read_to_string(&mut content)?;
    }

    Ok(parse_content(&content))
}

/// Parse an already-decompressed `SyncTeX` document.
///
/// Malformed records are skipped rather than failing the whole parse — a
/// partially readable map is more useful than none.
fn parse_content(content: &str) -> SyncTexData {
    let mut data = SyncTexData {
        version: 1,
        magnification: 1000,
        unit: 1,
        x_offset: 0,
        y_offset: 0,
        inputs: HashMap::new(),
        pages: HashMap::new(),
    };

    let mut lines = content.lines().peekable();

    while let Some(&line) = lines.peek() {
        if let Some(rest) = line.strip_prefix("Input:") {
            if let Some((id, path)) = rest.split_once(':') {
                if let Ok(id) = id.parse::<u32>() {
                    data.inputs.insert(id, path.to_owned());
                }
            }
        } else if line == "Content:" {
            lines.next();
            break;
        }
        lines.next();
    }

    while let Some(line) = lines.next() {
        if let Some(page_digits) = line.strip_prefix('{') {
            let page_num = page_digits.parse::<u32>().unwrap_or(1);
            let mut page = Page {
                num: page_num,
                elements: Vec::new(),
            };

            for record in lines.by_ref() {
                if record.starts_with('}') {
                    break;
                }
                if let Some(node) = parse_line(record) {
                    page.elements.push(node);
                }
            }
            data.pages.insert(page_num, page);
        } else if line.starts_with("Postamble:") {
            break;
        }
    }

    data
}

/// Field triple `W,H,D` (width, height, depth) in `SyncTeX` units.
#[derive(Default, Clone, Copy)]
struct Extent {
    width: i32,
    height: i32,
    depth: i32,
}

impl Extent {
    /// Parse the trailing `W,H,D` group; absent components default to 0.
    fn parse(field: &str) -> Self {
        let mut parts = field.split(',');
        Self {
            width: parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            height: parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            depth: parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        }
    }
}

/// Parse one content record. Returns `None` for blank, malformed, or
/// unrecognised-prefix lines.
fn parse_line(line: &str) -> Option<SyncTexNode> {
    let prefix = line.chars().next()?;
    let fields: Vec<&str> = line.get(1..)?.split(':').collect();
    if fields.len() < 3 {
        return None;
    }

    let (tag_str, row_str) = fields[0].split_once(',')?;
    let input_tag = tag_str.parse().unwrap_or(0);
    let source_line = row_str.parse().unwrap_or(0);
    let column = fields[1].parse().unwrap_or(0);

    let (x_field, y_field) = fields[2].split_once(',')?;
    let pos_x = x_field.parse().unwrap_or(0);
    let pos_y = y_field.parse().unwrap_or(0);

    let extent = fields
        .get(3)
        .map_or_else(Extent::default, |f| Extent::parse(f));

    let box_data = BoxData {
        link: input_tag,
        line: source_line,
        col: column,
        x: pos_x,
        y: pos_y,
        width: extent.width,
        height: extent.height,
        depth: extent.depth,
        children: Vec::new(),
    };
    let node_data = NodeData {
        link: input_tag,
        line: source_line,
        col: column,
        x: pos_x,
        y: pos_y,
        width: Some(extent.width),
    };

    match prefix {
        '[' => Some(SyncTexNode::HBox(box_data)),
        '(' => Some(SyncTexNode::VBox(box_data)),
        'v' => Some(SyncTexNode::VoidVBox(box_data)),
        'h' => Some(SyncTexNode::VoidHBox(box_data)),
        'g' => Some(SyncTexNode::Glue(node_data)),
        'k' => Some(SyncTexNode::Kern(node_data)),
        '$' => Some(SyncTexNode::Math(node_data)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "SyncTeX Version:1\n\
        Input:1:/tmp/main.tex\n\
        Input:2:/tmp/chapter.tex\n\
        Content:\n\
        {1\n\
        [1,10:100:200,300:400,50,10\n\
        (2,20:5:210,310:100,20,0\n\
        garbage line\n\
        }\n\
        Postamble:\n";

    #[test]
    fn parses_inputs_pages_and_records() {
        let data = parse_content(SAMPLE);
        assert_eq!(
            data.inputs.get(&1).map(String::as_str),
            Some("/tmp/main.tex")
        );
        assert_eq!(data.inputs.len(), 2);

        let page = data.pages.get(&1).expect("page 1 parsed");
        assert_eq!(page.elements.len(), 2, "unparseable line must be skipped");
    }

    #[test]
    fn reads_record_fields_positionally() {
        let node = parse_line("[1,10:100:200,300:400,50,10").expect("valid hbox");
        let SyncTexNode::HBox(b) = node else {
            panic!("expected an HBox")
        };
        assert_eq!((b.link, b.line, b.col), (1, 10, 100));
        assert_eq!((b.x, b.y), (200, 300));
        assert_eq!((b.width, b.height, b.depth), (400, 50, 10));
    }

    #[test]
    fn rejects_malformed_and_unknown_records() {
        assert!(parse_line("").is_none(), "empty line");
        assert!(parse_line("[1,10").is_none(), "too few fields");
        assert!(parse_line("?1,10:1:2,3:4,5,6").is_none(), "unknown prefix");
    }

    #[test]
    fn missing_extent_defaults_to_zero() {
        let node = parse_line("h1,10:5:20,30").expect("valid void hbox");
        let SyncTexNode::VoidHBox(b) = node else {
            panic!("expected a VoidHBox")
        };
        assert_eq!((b.width, b.height, b.depth), (0, 0, 0));
    }
}
