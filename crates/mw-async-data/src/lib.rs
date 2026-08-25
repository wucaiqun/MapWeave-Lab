//! Shared async data-task framework.
//!
//! Tokio hosts the runtime; each data kind is a [`DataJob`]. Phase A ships
//! [`DataJob::TileFetch`] / [`DataJob::ProviderInit`] only.

mod runtime;

pub use runtime::{DataJob, DataResult, DataTaskRuntime};

/// Default max concurrent tile HTTP/decode tasks.
pub const DEFAULT_TILE_FETCH_CONCURRENCY: usize = 8;
