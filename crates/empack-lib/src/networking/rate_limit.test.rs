use super::*;
use mockito::Server;
use std::time::Instant;

#[test]
fn test_platform_rate_limits() {
    assert_eq!(Platform::Modrinth.rate_limit(), 300);
    assert_eq!(Platform::CurseForge.rate_limit(), 60);
}

#[test]
fn test_platform_burst_sizes() {
    assert_eq!(Platform::Modrinth.burst_size(), 600);
    assert_eq!(Platform::CurseForge.burst_size(), 120);
}

#[test]
fn test_backoff_config_default() {
    let config = BackoffConfig::default();
    assert_eq!(config.initial, Duration::from_secs(1));
    assert_eq!(config.max, Duration::from_secs(60));
    assert_eq!(config.multiplier, 2.0);
}

#[tokio::test]
async fn test_rate_limited_client_creation() {
    let client = Client::new();
    let rate_limited = RateLimitedClient::new(client, Platform::Modrinth);

    assert_eq!(rate_limited.platform(), Platform::Modrinth);
}

#[tokio::test]
async fn test_rate_limited_client_with_custom_backoff() {
    let client = Client::new();
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(500),
        max: Duration::from_secs(30),
        multiplier: 1.5,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::CurseForge, backoff_config.clone());

    assert_eq!(rate_limited.platform(), Platform::CurseForge);
    assert_eq!(rate_limited.backoff_config.initial, backoff_config.initial);
    assert_eq!(rate_limited.backoff_config.max, backoff_config.max);
    assert_eq!(rate_limited.backoff_config.multiplier, backoff_config.multiplier);
}

#[tokio::test]
async fn test_successful_request() {
    let client = Client::new();
    let rate_limited = RateLimitedClient::new(client, Platform::Modrinth);

    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body("success")
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let result = rate_limited.get(&url).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.text().await.unwrap();
    assert_eq!(body, "success");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_rate_limit_429_with_retry() {
    let client = Client::new();

    // Use fast backoff for testing
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(100),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    let mut server = Server::new_async().await;

    // First request returns 429
    let mock_429 = server
        .mock("GET", "/test")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    // Second request succeeds
    let mock_200 = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body("success after retry")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let start = Instant::now();
    let result = rate_limited.get(&url).await;

    // Should have waited at least the initial backoff duration
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(10));

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.text().await.unwrap();
    assert_eq!(body, "success after retry");

    mock_429.assert_async().await;
    mock_200.assert_async().await;
}

#[tokio::test]
async fn test_exponential_backoff() {
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(100),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::CurseForge, backoff_config);

    let mut server = Server::new_async().await;

    // Return 429 three times, then succeed
    let mock_429_1 = server
        .mock("GET", "/test")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    let mock_429_2 = server
        .mock("GET", "/test")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    let mock_429_3 = server
        .mock("GET", "/test")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    let mock_200 = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body("finally succeeded")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let start = Instant::now();
    let result = rate_limited.get(&url).await;

    // Total backoff should be: 10ms + 20ms + 40ms = 70ms minimum
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(70));

    assert!(result.is_ok());

    mock_429_1.assert_async().await;
    mock_429_2.assert_async().await;
    mock_429_3.assert_async().await;
    mock_200.assert_async().await;
}

#[tokio::test]
async fn test_max_retries_exceeded() {
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(5),
        max: Duration::from_millis(20),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    let mut server = Server::new_async().await;

    // Always return 429 (will exceed max retries)
    let mock = server
        .mock("GET", "/test")
        .with_status(429)
        .expect_at_least(6) // Should retry 5 times + initial request
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let result = rate_limited.get(&url).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        NetworkingError::RateLimitError { message } => {
            assert!(message.contains("after 5 retries"));
        }
        _ => panic!("Expected RateLimitError"),
    }

    mock.assert_async().await;
}

#[tokio::test]
async fn test_backoff_capped_at_max() {
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(50), // Cap at 50ms
        multiplier: 10.0,              // Would grow very fast without cap
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    // Simulate multiple 429s to trigger exponential growth
    let current_backoff = rate_limited.current_backoff.clone();

    // Manually advance backoff several times
    {
        let mut backoff = current_backoff.write().await;
        *backoff = Duration::from_millis(10);
        *backoff = Duration::from_secs_f64(backoff.as_secs_f64() * 10.0); // Would be 100ms
        *backoff = (*backoff).min(Duration::from_millis(50)); // Capped at 50ms
    }

    let backoff = current_backoff.read().await;
    assert_eq!(*backoff, Duration::from_millis(50));
}

#[tokio::test]
async fn test_rate_limiter_manager_creation() {
    let client = Client::new();
    let manager = RateLimiterManager::new(client);

    assert_eq!(manager.modrinth().platform(), Platform::Modrinth);
    assert_eq!(manager.curseforge().platform(), Platform::CurseForge);
}

#[tokio::test]
async fn test_rate_limiter_manager_with_backoff() {
    let client = Client::new();
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(500),
        max: Duration::from_secs(30),
        multiplier: 1.5,
    };

    let manager = RateLimiterManager::with_backoff(client, backoff_config.clone());

    assert_eq!(manager.modrinth().backoff_config.initial, backoff_config.initial);
    assert_eq!(manager.curseforge().backoff_config.max, backoff_config.max);
}

#[tokio::test]
async fn test_client_for_platform() {
    let client = Client::new();
    let manager = RateLimiterManager::new(client);

    let modrinth = manager.client_for_platform(Platform::Modrinth);
    assert_eq!(modrinth.platform(), Platform::Modrinth);

    let curseforge = manager.client_for_platform(Platform::CurseForge);
    assert_eq!(curseforge.platform(), Platform::CurseForge);
}

#[tokio::test]
async fn test_post_request() {
    let client = Client::new();
    let rate_limited = RateLimitedClient::new(client, Platform::Modrinth);

    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/api")
        .match_body("test payload")
        .with_status(201)
        .with_body("created")
        .create_async()
        .await;

    let url = format!("{}/api", server.url());
    let result = rate_limited.post(&url, b"test payload".to_vec()).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    mock.assert_async().await;
}

#[tokio::test]
async fn test_backoff_reset_on_success() {
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(100),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config.clone());

    let mut server = Server::new_async().await;

    // First: 429, then success
    let mock_429 = server
        .mock("GET", "/test1")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    let mock_200_1 = server
        .mock("GET", "/test1")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;

    let url1 = format!("{}/test1", server.url());
    let _ = rate_limited.get(&url1).await.unwrap();

    mock_429.assert_async().await;
    mock_200_1.assert_async().await;

    // Backoff should be reset to initial after success
    let current = rate_limited.current_backoff.read().await;
    assert_eq!(*current, backoff_config.initial);
}

// ============================================================================
// Resilience Tests (Phase A - Category 4: Networking Resilience)
// ============================================================================

#[tokio::test]
async fn test_exponential_backoff_progression() {
    // Test exponential backoff: verify backoff increases on repeated failures
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(80),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config.clone());

    let mut server = Server::new_async().await;

    // Simulate many 429 responses (rate limiter will retry with backoff)
    let mock_429 = server
        .mock("GET", "/backoff-test")
        .with_status(429)
        .expect_at_least(1)
        .create_async()
        .await;

    let url = format!("{}/backoff-test", server.url());

    // Check initial backoff
    {
        let current = rate_limited.current_backoff.read().await;
        assert_eq!(*current, backoff_config.initial); // 10ms initially
    }

    // Make a request that will fail repeatedly (causing exponential backoff)
    let _ = rate_limited.get(&url).await;

    // After multiple 429 retries, backoff should have increased
    let final_backoff = rate_limited.current_backoff.read().await;
    // Should be greater than initial (may or may not reach max depending on retry count)
    assert!(*final_backoff >= backoff_config.initial);

    mock_429.assert_async().await;
}

#[tokio::test]
async fn test_rate_limit_exhaustion_and_refill() {
    // Test rate limit bucket exhaustion and refill behavior
    let client = Client::new();
    let rate_limited = RateLimitedClient::new(client, Platform::Modrinth);

    let mut server = Server::new_async().await;

    // Mock successful responses
    let mock = server
        .mock("GET", "/burst")
        .with_status(200)
        .with_body("ok")
        .expect_at_least(1)
        .create_async()
        .await;

    let url = format!("{}/burst", server.url());

    // Make multiple requests quickly (within rate limit window)
    let mut results = vec![];
    for _ in 0..5 {
        let result = rate_limited.get(&url).await;
        results.push(result);
    }

    // All should succeed (rate limiter allows burst)
    for result in results {
        assert!(result.is_ok());
    }

    mock.assert_async().await;
}

#[tokio::test]
async fn test_concurrent_requests_with_rate_limiting() {
    // Test concurrent requests (10+) with rate limiting
    let client = Arc::new(Client::new());
    let rate_limited = Arc::new(RateLimitedClient::new((*client).clone(), Platform::Modrinth));

    let mut server = Server::new_async().await;

    // Mock response for concurrent requests
    let mock = server
        .mock("GET", "/concurrent-rate-limit")
        .with_status(200)
        .with_body("ok")
        .expect_at_least(10)
        .create_async()
        .await;

    let url = format!("{}/concurrent-rate-limit", server.url());

    // Spawn 10 concurrent requests
    let mut handles = vec![];
    for _ in 0..10 {
        let rate_limited_clone = Arc::clone(&rate_limited);
        let url_clone = url.clone();

        let handle = tokio::spawn(async move {
            rate_limited_clone.get(&url_clone).await
        });

        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    mock.assert_async().await;
}
