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

    assert!(manager.optimal_jobs() <= 2, "Should respect max jobs limit");
    assert!(manager.optimal_jobs() > 0, "Should have at least 1 job");
}

#[tokio::test]
async fn test_resolve_mods_success_and_error_results() {
    let config = NetworkingConfig {
        max_jobs: Some(4),
        trace_requests: true,
        ..Default::default()
    };
    let manager = NetworkingManager::new(config).await.unwrap();

    let results = manager
        .resolve_mods(
            vec!["alpha".to_string(), "beta".to_string()],
            |client, mod_id| async move {
                let _ = client.get("https://example.com");
                match mod_id.as_str() {
                    "alpha" => Ok(format!("resolved-{mod_id}")),
                    "beta" => Err(NetworkingError::RateLimitError {
                        message: "simulated failure".to_string(),
                    }),
                    _ => Ok(mod_id),
                }
            },
        )
        .await
        .expect("resolve mods");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_ref().expect("alpha"), "resolved-alpha");
    assert!(matches!(
        results[1],
        Err(NetworkingError::RateLimitError { .. })
    ));
    assert!(manager.client().get("https://example.com").build().is_ok());
}
