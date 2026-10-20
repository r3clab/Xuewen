#![warn(unreachable_pub)]
// TODO: drop once the cli layer lands; until then lower layers hold
// helpers whose callers are not written yet.
#![allow(dead_code, unused_imports)]

mod annotations;
pub mod citations;
pub mod config;
pub mod daily;
pub mod db;
mod hash;
mod http;
pub mod library;
pub mod llm;
pub mod metadata;
mod pdf;
pub mod preview;
pub mod search;
pub mod summary;
pub mod tasks;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testutil;
mod text;
pub mod translate;
