//! Ruff's database API uses `web-time` on wasm32, which normally imports a JS clock.
//! Embedded linting needs no clock. Use std's types so core-WASM stays import-free.
//! On wasm32-unknown-unknown, unexpected `now()` calls panic instead of calling a host.
pub use std::time::*;
