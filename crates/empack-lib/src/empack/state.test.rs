use super::*;

use tempfile::TempDir;

fn create_test_manager() -> (TempDir, ModpackStateManager) {
    let temp_dir = TempDir::new().unwrap();
    let manager = ModpackStateManager::new(temp_dir.path().to_path_buf());
    (temp_dir, manager)
}

#[test]
fn test_initial_state_is_uninitialized() {
    let (_temp, manager) = create_test_manager();
    let state = manager.discover_state().unwrap();
    assert_eq!(state, ModpackState::Uninitialized);
}

#[test]
fn test_transition_to_configured() {
    let (_temp, manager) = create_test_manager();

    let result = manager
        .execute_transition(StateTransition::Initialize)
        .unwrap();
    assert_eq!(result, ModpackState::Configured);

    // Verify files were created
    let paths = manager.paths();
    assert!(paths.empack_yml.exists());
    assert!(paths.pack_dir.exists());
    assert!(paths.template_dir.exists());
}

#[test]
fn test_transition_to_built() {
    let (_temp, manager) = create_test_manager();

    // Initialize first
    manager
        .execute_transition(StateTransition::Initialize)
        .unwrap();

    // Then build
    let targets = vec![BuildTarget::Mrpack, BuildTarget::Client];
    let result = manager
        .execute_transition(StateTransition::Build(targets))
        .unwrap();
    assert_eq!(result, ModpackState::Built);

    // Verify dist directory was created
    let paths = manager.paths();
    assert!(paths.dist_dir.exists());
    assert!(paths.build_output(BuildTarget::Mrpack).exists());
    assert!(paths.build_output(BuildTarget::Client).exists());
}

#[test]
fn test_clean_transitions() {
    let (_temp, manager) = create_test_manager();

    // Build up to built state
    manager
        .execute_transition(StateTransition::Initialize)
        .unwrap();

    manager
        .execute_transition(StateTransition::Build(vec![BuildTarget::Mrpack]))
        .unwrap();

    // Clean back to configured
    let result = manager.execute_transition(StateTransition::Clean).unwrap();
    assert_eq!(result, ModpackState::Configured);
    assert!(!manager.paths().dist_dir.exists());

    // Clean back to uninitialized
    let result = manager.execute_transition(StateTransition::Clean).unwrap();
    assert_eq!(result, ModpackState::Uninitialized);
    assert!(!manager.paths().empack_yml.exists());
    assert!(!manager.paths().pack_dir.exists());
}

#[test]
fn test_invalid_transitions() {
    let (_temp, manager) = create_test_manager();

    // Can't build from uninitialized
    let result = manager.execute_transition(StateTransition::Build(vec![BuildTarget::Mrpack]));
    assert!(result.is_err());

    // Can't sync from uninitialized
    let result = manager.execute_transition(StateTransition::Synchronize);
    assert!(result.is_err());
}

#[test]
fn test_state_validation() {
    let (_temp, manager) = create_test_manager();

    // Uninitialized should validate correctly
    assert!(manager.validate_state(ModpackState::Uninitialized).unwrap());
    assert!(!manager.validate_state(ModpackState::Configured).unwrap());

    // After initialization, configured should validate
    manager
        .execute_transition(StateTransition::Initialize)
        .unwrap();
    assert!(manager.validate_state(ModpackState::Configured).unwrap());
    assert!(!manager.validate_state(ModpackState::Uninitialized).unwrap());
}

#[test]
fn test_paths_helper() {
    let (_temp, manager) = create_test_manager();
    let paths = manager.paths();

    assert_eq!(paths.empack_yml, manager.workdir.join("empack.yml"));
    assert_eq!(
        paths.pack_toml,
        manager.workdir.join("pack").join("pack.toml")
    );
    assert_eq!(
        paths.build_output(BuildTarget::Mrpack),
        manager.workdir.join("dist").join("mrpack")
    );
}
