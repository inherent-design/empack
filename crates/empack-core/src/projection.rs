//! Deterministic build prerequisites, independent of tool execution and codecs.
use alloc::vec::Vec;

/// A semantic distribution target; CLI and document spellings belong to adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildTarget {
    /// Reference-based Modrinth archive.
    Mrpack,
    /// Client bootstrap distribution.
    Client,
    /// Server bootstrap distribution.
    Server,
    /// Client distribution containing acquired content.
    ClientFull,
    /// Server distribution containing acquired content.
    ServerFull,
}

/// Expand fresh prerequisites once, preserving requested target order where possible.
/// This plan describes ordering only; it does not prove input or artifact integrity.
pub fn plan_build_targets(targets: &[BuildTarget]) -> Vec<BuildTarget> {
    let mut ordered = Vec::new();
    for target in targets {
        if matches!(target, BuildTarget::Client | BuildTarget::Server)
            && !ordered.contains(&BuildTarget::Mrpack)
        {
            ordered.push(BuildTarget::Mrpack);
        }
        if !ordered.contains(target) {
            ordered.push(*target);
        }
    }
    ordered
}
