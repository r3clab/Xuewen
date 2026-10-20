#![warn(unreachable_pub)]
// TODO: drop once the cli layer lands; until then lower layers hold
// helpers whose callers are not written yet.
#![allow(dead_code, unused_imports)]

pub mod config;
mod hash;
mod http;
mod pdf;
pub mod tasks;
mod text;
