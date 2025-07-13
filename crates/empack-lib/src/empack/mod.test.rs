use super::*;

// Tests for empack module coordination and re-exports
#[test]
fn test_module_reexports() {
    // Verify main types are accessible through re-exports
    let _transition = StateTransition::Initialize;
    let _state = ModpackState::Uninitialized;
    let _target = BuildTarget::Mrpack;
    let _project_type = ProjectType::Mod;
}
