//! Typed engine boundaries. Native execution is composed separately from document decoding.
//!
//! Project mutation is available through [`api::Engine`] preparation and authorization.
//! Publication and proof construction are internal implementation details.
//!
//! ```compile_fail
//! use empack_lib::engine::publication::Publisher;
//! ```
//! ```compile_fail
//! use empack_lib::engine::staging::MutableStage;
//! ```
//! ```compile_fail
//! use empack_lib::engine::verification::VerifiedFileChange;
//! ```
//! ```
//! use empack_lib::engine::{api::Engine, publication::PublicationReceipt};
//! fn supported_boundary(_: &Engine, _: &PublicationReceipt) {}
//! ```
pub mod acquisition;
pub mod api;
pub mod archive_source;
pub mod artifacts;
pub mod build;
pub mod content;
mod continuation_store;
pub mod dependency_content;
pub mod diagnostics;
pub mod documents;

pub mod addition;
pub mod import;
pub mod initialize;
pub mod instance;
mod io;
pub mod layout;
pub mod mrpack;
mod native;
pub mod project;
pub mod project_change;
pub mod providers;
pub mod publication;
pub mod release;
pub mod removal;
pub mod resources;
pub mod retained_cleanup;
pub mod runtime;
pub mod runtime_catalog;
pub mod server_runtime;
pub mod snapshot;
pub mod source;
mod staging;
pub mod synchronization;
pub mod templates;
mod verification;

#[cfg(windows)]
mod windows_privacy;

pub(crate) mod runtime_versions;
