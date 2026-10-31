//! Async Virse client. Configuration, persistence and terminal output belong to callers.
mod asset_types;
mod assets;
mod changes;
mod client;
pub mod compatibility;
pub use asset_types::*;
pub use changes::*;
mod error;
mod generation;
mod image_edit;
pub use image_edit::{MAX_IMAGE_MASK_BYTES, UploadedImageMask};
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

mod share_address;
mod share_types;
mod sharing;
pub use share_address::{ShareAddress, absolute_share_url};
pub use share_types::*;

mod tool_authoring;
mod tool_runs;
mod tool_types;
mod tools;
pub use tool_authoring::*;
pub use tool_types::*;

mod discovery;
pub use discovery::*;

mod shortcuts;
pub use shortcuts::*;

mod workflows;
pub use workflows::*;

mod aesthetics;
pub use aesthetics::*;

mod composer;
pub use composer::*;
