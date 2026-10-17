//! Use-case-agnostic services (masterplan.md §4.5): storage, security,
//! AI planning, edit application, fuzzy search, prompt safety, metrics.
#![forbid(unsafe_code)]
// Tests build strings from iterators and index with small known values;
// those pedantic lints are noise there.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::format_collect,
        clippy::cast_possible_truncation
    )
)]

pub mod ai;
pub mod apply;
pub mod compile;
pub mod config;
pub mod fsutil;
pub mod metrics;
pub mod policy;
pub mod project;
pub mod prompt_safety;
pub mod search;
pub mod security;
pub mod storage;
