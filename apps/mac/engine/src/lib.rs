//! Open Notebook's engine for the Rune Mac app: a Rust port of `packages/engine`
//! (on-device backend) and `packages/api-client` (server backend).

pub mod backend;
pub mod error;
pub mod extract;
pub mod net;
pub mod remote;
pub mod settings;
pub mod transcribe;
pub mod types;
pub mod youtube;

pub use backend::{Backend, BackendKind, Cancel, PodcastMode, Result};
pub use error::Error;
