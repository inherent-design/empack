//! Typed engine boundaries. Native execution is composed separately from document decoding.
pub mod documents;

mod io;
mod native;
pub mod snapshot;
pub mod staging;
