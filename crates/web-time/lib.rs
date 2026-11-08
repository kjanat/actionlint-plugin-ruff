//! Supply `std::time` types for Ruff without importing a JavaScript clock.
//! Linting does not need the clock; `now()` panics on wasm32-unknown-unknown.
pub use std::time::*;
