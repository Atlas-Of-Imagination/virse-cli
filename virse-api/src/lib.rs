//! Async Virse client. Configuration, persistence and terminal output belong to callers.
mod client;
mod error;
mod generation;
mod types;

pub use client::{Client, normalize_backend};
pub use error::{Error, Result};
pub use types::*;
