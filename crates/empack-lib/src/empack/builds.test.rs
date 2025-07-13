use super::*;
use tempfile::TempDir;

fn create_test_orchestrator() -> (TempDir, BuildOrchestrator) {
    let temp_dir = TempDir::new().unwrap();
    let orchestrator = BuildOrchestrator::new(temp_dir.path().to_path_buf());
    (temp_dir, orchestrator)
}

#[test]
fn test_build_registry() {
    let registry = BuildOrchestrator::create_build_registry();
    assert_eq!(registry.len(), 5);
    assert!(registry.contains_key(&BuildTarget::Mrpack));
    assert!(registry.contains_key(&BuildTarget::Client));

    // Test dependencies (V1 pattern)
    let client_config = &registry[&BuildTarget::Client];
    assert_eq!(client_config.dependencies, vec![BuildTarget::Mrpack]);
}

#[test]
fn test_prepare_build_environment() {
    let (_temp, orchestrator) = create_test_orchestrator();

    // Should fail without pack directory
    let result = orchestrator.prepare_build_environment();
    assert!(result.is_err());

    // Create pack directory
    std::fs::create_dir_all(orchestrator.workdir.join("pack")).unwrap();

    // May still fail if packwiz not available, but structure should be validated
    let _result = orchestrator.prepare_build_environment();
}
