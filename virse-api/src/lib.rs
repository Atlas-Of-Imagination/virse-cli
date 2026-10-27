//! Async Virse client. Configuration, persistence and terminal output belong to callers.
mod client;
mod error;
mod generation;
mod resource_types;
mod resources;
mod types;

pub use client::{Client, normalize_backend};
pub use error::{Error, Result};
pub use resource_types::*;
pub use types::*;

mod canvas;
mod media;
pub use canvas::*;
pub use media::MediaKind;
