//! # API Module
//!
//! Platform API abstraction layer for dependency resolution and project management.
//!
//! ## Modules
//!
//! - [`dependency_graph`] - Transitive dependency resolution with cycle detection

pub mod dependency_graph;

pub use dependency_graph::{DependencyGraph, DependencyGraphError, DependencyNode};
