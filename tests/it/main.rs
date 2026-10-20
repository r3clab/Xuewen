// TODO: drop once the cli layer lands; until then lower layers hold
// helpers whose callers are not written yet.
#![allow(dead_code, unused_imports)]

mod support;

mod agent;
mod grobid;
mod resolve;
mod schema;
