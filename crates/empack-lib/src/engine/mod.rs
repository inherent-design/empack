//! Typed engine boundaries. Native execution is composed separately from document decoding.
pub mod acquisition;
pub mod api;
pub mod archive_source;
pub mod artifacts;
pub mod backend;
pub mod bootstrap_tools;
pub mod build;
pub mod content;
pub mod documents;

pub mod import;
pub mod initialize;
mod io;
pub mod layout;
pub mod mrpack;
mod native;
pub mod packwiz;
pub mod project;
pub mod project_change;
pub mod providers;
pub mod publication;
pub mod removal;
pub mod resources;
pub mod runtime;
pub mod server_runtime;
pub mod snapshot;
pub mod source;
pub mod staging;
pub mod templates;
pub mod verification;

#[cfg(windows)]
mod windows_privacy;
