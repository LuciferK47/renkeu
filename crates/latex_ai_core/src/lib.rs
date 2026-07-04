//! Use-case-agnostic services (M2).
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod search;
pub mod storage;
pub mod compile;
pub mod ai;
pub mod prompt_safety;
pub mod security;
