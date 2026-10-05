//! Typed engine boundaries. Native execution is composed separately from document decoding.
pub mod documents;

mod io;
pub mod layout;
mod native;
pub mod publication;
pub mod resources;
pub mod snapshot;
pub mod staging;
pub mod verification;

#[cfg(windows)]
mod windows_privacy;
