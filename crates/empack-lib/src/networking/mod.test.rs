use super::*;

#[tokio::test]
async fn test_networking_manager_creation() {
    let config = NetworkingConfig::default();
    let manager = NetworkingManager::new(config).await;

    assert!(manager.is_ok(), "Should create networking manager");
    let manager = manager.unwrap();
    assert!(
        manager.optimal_jobs() > 0,
        "Should calculate positive job count"
    );
}

#[tokio::test]
async fn test_empty_mod_list_error() {
    let config = NetworkingConfig::default();
    let manager = NetworkingManager::new(config).await.unwrap();

    let result = manager
        .resolve_mods(Vec::<String>::new(), |_client, _mod_id| async {
            Ok("test".to_string())
        })
        .await;

    assert!(matches!(result, Err(NetworkingError::NoModsProvided)));
}

#[tokio::test]
async fn test_job_calculation_with_limit() {
    let config = NetworkingConfig {
        max_jobs: Some(2),
        ..Default::default()
    };
    let manager = NetworkingManager::new(config).await.unwrap();

    // Should respect the max_jobs limit
    assert!(manager.optimal_jobs() <= 2, "Should respect max jobs limit");
    assert!(manager.optimal_jobs() > 0, "Should have at least 1 job");
}

#[test]
fn test_mock_mod_resolution() {
    // Test the resolver function signature works
    let mock_resolver = |_client: Client, mod_id: String| async move {
        Ok::<String, NetworkingError>(format!("resolved_{}", mod_id))
    };

    // This just verifies the function signature compiles
    let _test: fn(Client, String) -> _ = mock_resolver;
}
