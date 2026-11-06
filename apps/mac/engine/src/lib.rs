//! Open Notebook's engine for the Rune Mac app: a Rust port of `packages/engine`
//! (on-device backend) and `packages/api-client` (server backend).

pub mod backend;
pub mod error;
pub mod settings;
pub mod types;

pub use backend::{Backend, BackendKind, Cancel, PodcastMode, Result};
pub use error::Error;
