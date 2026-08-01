//! `SyncTeX` source-map reader: parses `.synctex(.gz)` and maps PDF
//! coordinates back to source positions. Format parsing only — this crate has
//! zero knowledge of LaTeX semantics (masterplan.md §5 rule 7).
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::HashMap;
use std::path::Path;

pub mod parser;
pub mod search;

#[derive(Debug, thiserror::Error)]
pub enum SyncTexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("File not found in SyncTeX data")]
    FileNotFound,
}

#[derive(Debug, Clone)]
pub struct SyncTexData {
    pub version: u32,
    pub magnification: u32,
    pub unit: u32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub inputs: HashMap<u32, String>,
    pub pages: HashMap<u32, Page>,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub num: u32,
    pub elements: Vec<SyncTexNode>,
}

#[derive(Debug, Clone)]
pub enum SyncTexNode {
    HBox(BoxData),
    VBox(BoxData),
    VoidHBox(BoxData),
    VoidVBox(BoxData),
    Math(NodeData),
    Glue(NodeData),
    Kern(NodeData),
}

#[derive(Debug, Clone)]
pub struct BoxData {
    pub link: u32,
    pub line: u32,
    pub col: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub children: Vec<SyncTexNode>,
}

#[derive(Debug, Clone)]
pub struct NodeData {
    pub link: u32,
    pub line: u32,
    pub col: u32,
    pub x: i32,
    pub y: i32,
    pub width: Option<i32>,
}

impl SyncTexData {
    pub fn parse(path: &Path) -> Result<Self, SyncTexError> {
        parser::parse_file(path)
    }

    pub fn backward_search(&self, page: u32, x: i32, y: i32) -> Option<search::SourceHit> {
        search::backward_search(self, page, x, y)
    }
}
