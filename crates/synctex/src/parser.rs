use crate::{BoxData, NodeData, Page, SyncTexData, SyncTexError, SyncTexNode};
use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;




// Just a basic fallback parser for demonstration.
// In reality, SyncTeX parsing via nom would require carefully matching the nested structure.
// To keep things simple and robust for this milestone, we will do a line-by-line recursive descent parse.
pub fn parse_file(path: &Path) -> Result<SyncTexData, SyncTexError> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    
    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        let mut gz = GzDecoder::new(file);
        gz.read_to_string(&mut content).map_err(|e| SyncTexError::Io(e))?;
    } else {
        file.read_to_string(&mut content)?;
    }

    parse_content(&content)
}

fn parse_content(content: &str) -> Result<SyncTexData, SyncTexError> {
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
    
    // Parse preamble
    while let Some(&line) = lines.peek() {
        if line.starts_with("Input:") {
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() == 3 {
                if let Ok(id) = parts[1].parse::<u32>() {
                    data.inputs.insert(id, parts[2].to_string());
                }
            }
        } else if line == "Content:" {
            lines.next();
            break;
        }
        lines.next();
    }

    // Parse pages
    while let Some(line) = lines.next() {
        if line.starts_with('{') {
            let page_num = line[1..].parse::<u32>().unwrap_or(1);
            let mut page = Page {
                num: page_num,
                elements: Vec::new(),
            };
            
            // Collect page elements
            while let Some(pline) = lines.next() {
                if pline.starts_with('}') {
                    break;
                }
                
                // Very simplified parser for backward search purposes
                // Real SyncTeX contains nested trees, we just flatten it for backward search
                if let Some(node) = parse_line(pline) {
                    page.elements.push(node);
                }
            }
            data.pages.insert(page_num, page);
        } else if line.starts_with("Postamble:") {
            break;
        }
    }

    Ok(data)
}

fn parse_line(line: &str) -> Option<SyncTexNode> {
    if line.is_empty() { return None; }
    let prefix = line.chars().next()?;
    
    // Format is typically: [link,line:col:x,y:W,H,D
    // Or (link,line:col:x,y:W,H,D
    let content = &line[1..];
    let parts: Vec<&str> = content.split(':').collect();
    if parts.len() < 3 { return None; }
    
    let link_line: Vec<&str> = parts[0].split(',').collect();
    if link_line.len() < 2 { return None; }
    let link = link_line[0].parse().unwrap_or(0);
    let line_num = link_line[1].parse().unwrap_or(0);
    
    let col = parts[1].parse().unwrap_or(0);
    
    let xy: Vec<&str> = parts[2].split(',').collect();
    if xy.len() < 2 { return None; }
    let x = xy[0].parse().unwrap_or(0);
    let y = xy[1].parse().unwrap_or(0);
    
    let mut w = 0;
    let mut h = 0;
    let mut d = 0;
    
    if parts.len() >= 4 {
        let whd: Vec<&str> = parts[3].split(',').collect();
        if whd.len() >= 1 { w = whd[0].parse().unwrap_or(0); }
        if whd.len() >= 2 { h = whd[1].parse().unwrap_or(0); }
        if whd.len() >= 3 { d = whd[2].parse().unwrap_or(0); }
    }
    
    let bdata = BoxData { link, line: line_num, col, x, y, width: w, height: h, depth: d, children: vec![] };
    let ndata = NodeData { link, line: line_num, col, x, y, width: Some(w) };
    
    match prefix {
        '[' => Some(SyncTexNode::HBox(bdata)),
        '(' => Some(SyncTexNode::VBox(bdata)),
        'v' => Some(SyncTexNode::VoidVBox(bdata)),
        'h' => Some(SyncTexNode::VoidHBox(bdata)),
        'g' => Some(SyncTexNode::Glue(ndata)),
        'k' => Some(SyncTexNode::Kern(ndata)),
        '$' => Some(SyncTexNode::Math(ndata)),
        _ => None,
    }
}
