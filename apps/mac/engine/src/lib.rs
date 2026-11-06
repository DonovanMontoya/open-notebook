//! Open Notebook's engine for the Rune Mac app: a Rust port of `packages/engine`
//! (on-device backend) and `packages/api-client` (server backend).

pub mod ask;
pub mod backend;
pub mod blocks;
pub mod chat;
pub mod citations;
pub mod db;
pub mod error;
pub mod extract;
pub mod insights;
pub mod net;
pub mod pricing;
pub mod remote;
pub mod settings;
pub mod studio;
pub mod text;
pub mod transcribe;
pub mod types;
pub mod youtube;

pub use backend::{Backend, BackendKind, Cancel, PodcastMode, Result};
pub use error::Error;
