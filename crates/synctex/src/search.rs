use crate::{SyncTexData, SyncTexNode};

#[derive(Debug, Clone)]
pub struct SourceHit {
    pub file: String,
    pub line: u32,
    pub col: u32,
}

pub fn backward_search(data: &SyncTexData, page_num: u32, x: i32, y: i32) -> Option<SourceHit> {
    let page = data.pages.get(&page_num)?;

    let mut best_hit = None;
    let mut min_dist = i32::MAX;

    for element in &page.elements {
        let (tag, row, column, box_x, box_y, box_w, box_h) = match element {
            SyncTexNode::HBox(b)
            | SyncTexNode::VBox(b)
            | SyncTexNode::VoidHBox(b)
            | SyncTexNode::VoidVBox(b) => (b.link, b.line, b.col, b.x, b.y, b.width, b.height),
            SyncTexNode::Math(n) | SyncTexNode::Glue(n) | SyncTexNode::Kern(n) => {
                (n.link, n.line, n.col, n.x, n.y, n.width.unwrap_or(0), 0)
            }
        };

        if tag == 0 || row == 0 {
            continue;
        }

        let dx = if x < box_x {
            box_x - x
        } else if x > box_x + box_w {
            x - (box_x + box_w)
        } else {
            0
        };
        let dy = if y < box_y - box_h {
            box_y - box_h - y
        } else if y > box_y {
            y - box_y
        } else {
            0
        }; // y in SyncTex is usually baseline
        let dist = dx.saturating_add(dy);

        if dist < min_dist {
            min_dist = dist;
            if let Some(file) = data.inputs.get(&tag) {
                best_hit = Some(SourceHit {
                    file: file.clone(),
                    line: row,
                    col: column,
                });
            }
        }
    }

    best_hit
}
