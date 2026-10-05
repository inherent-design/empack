//! Pure semantic values and planners. No runtime, native I/O or document codecs.
#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

pub mod digest;
pub mod files;
pub mod identity;
pub mod model;
pub mod path;
pub mod projection;

pub mod requirements;
