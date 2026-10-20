#![warn(unreachable_pub)]

pub mod agent;
mod annotations;
pub mod app;
pub mod citations;
pub mod cli;
pub mod config;
pub mod daily;
pub mod db;
mod hash;
mod http;
pub mod ingest;
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
pub mod web;
