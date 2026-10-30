//! External verification support; never linked into the product executable.
pub mod archive;
#[cfg(target_os = "macos")]
pub mod native_volume;
pub mod process;
pub mod terminal;
