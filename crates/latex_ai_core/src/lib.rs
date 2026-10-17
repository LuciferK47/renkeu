//! Use-case-agnostic services (masterplan.md §4.5): storage, security,
//! AI planning, edit application, fuzzy search, prompt safety, metrics.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

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
