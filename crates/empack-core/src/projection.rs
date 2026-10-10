//! Deterministic build selection, independent of tool execution and codecs.
use alloc::vec::Vec;

/// A semantic distribution target; CLI and document spellings belong to adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildTarget {
    /// Reference-based Modrinth archive.
    Mrpack,
    /// CurseForge manifest with exact provider references and authored overrides.
    CurseForge,
    /// Client bootstrap distribution.
    Client,
    /// Server bootstrap distribution.
    Server,
    /// Client distribution containing acquired content.
    ClientFull,
    /// Server distribution containing acquired content.
    ServerFull,
}

/// Deduplicate recipes while preserving requested order. Recipes own their inputs.
/// This plan describes ordering only; it does not prove input or artifact integrity.
pub fn plan_build_targets(targets: &[BuildTarget]) -> Vec<BuildTarget> {
    let mut ordered = Vec::new();
    for target in targets {
        if !ordered.contains(target) {
            ordered.push(*target);
        }
    }
    ordered
}
