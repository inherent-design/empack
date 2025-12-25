// Tests for network retry behavior, rate limiting, and exponential backoff

use super::*;
use mockito::Server;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_exponential_backoff_calculation() {
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(100),
        max: Duration::from_secs(10),
        multiplier: 2.0,
    };

    // First backoff should be initial
    let mut current = backoff_config.initial;
    assert_eq!(current, Duration::from_millis(100));

    // Second backoff should be initial * multiplier
    current = Duration::from_millis((current.as_millis() as f64 * backoff_config.multiplier) as u64);
    assert_eq!(current, Duration::from_millis(200));

    // Third backoff should be 400ms
    current = Duration::from_millis((current.as_millis() as f64 * backoff_config.multiplier) as u64);
    assert_eq!(current, Duration::from_millis(400));

    // Verify backoff doesn't exceed max
    for _ in 0..10 {
        current = Duration::from_millis((current.as_millis() as f64 * backoff_config.multiplier) as u64);
        if current > backoff_config.max {
            current = backoff_config.max;
        }
    }
    assert_eq!(current, backoff_config.max);
}

#[tokio::test]
async fn test_retry_on_429_with_exponential_backoff() {
    let client = Client::new();

    // Use fast backoff for testing
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(100),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    let mut server = Server::new_async().await;

    // First two requests return 429, third succeeds
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

    let mock_200 = server
        .mock("GET", "/test")
        .with_status(200)
        .with_body("success")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let start = Instant::now();
    let result = rate_limited.get(&url).await;

    // Should succeed after retries
    assert!(result.is_ok(), "Should succeed after retries");
    let response = result.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify backoff delay occurred (at least 10ms + 20ms = 30ms for 2 retries)
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(30),
        "Backoff delay should have occurred: {:?}",
        elapsed
    );

    // Verify all mocks were called
    mock_429_1.assert_async().await;
    mock_429_2.assert_async().await;
    mock_200.assert_async().await;
}

#[tokio::test]
async fn test_retry_limit_exhaustion() {
    let client = Client::new();

    // Use fast backoff for testing
    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(1),
        max: Duration::from_millis(10),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    let mut server = Server::new_async().await;

    // Always return 429 (exceeds MAX_RETRIES = 5)
    let mock_429 = server
        .mock("GET", "/test")
        .with_status(429)
        .expect_at_least(6) // Initial + 5 retries
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let result = rate_limited.get(&url).await;

    // Should fail after exhausting retries
    assert!(result.is_err(), "Should fail after exhausting retries");

    if let Err(NetworkingError::RateLimitError { message }) = result {
        assert!(
            message.contains("Rate limit exceeded") || message.contains("after"),
            "Error should mention rate limit exceeded: {}",
            message
        );
        assert!(
            message.contains("Modrinth") || message.contains("modrinth"),
            "Error should mention platform: {}",
            message
        );
    } else {
        panic!("Expected RateLimitError, got: {:?}", result);
    }

    mock_429.assert_async().await;
}

#[tokio::test]
async fn test_backoff_reset_after_success() {
    let client = Client::new();

    let backoff_config = BackoffConfig {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(100),
        multiplier: 2.0,
    };

    let rate_limited = RateLimitedClient::with_backoff(client, Platform::Modrinth, backoff_config);

    let mut server = Server::new_async().await;

    // First request: 429 then success (should backoff)
    let mock_429_1 = server
        .mock("GET", "/test1")
        .with_status(429)
        .expect(1)
        .create_async()
        .await;

    let mock_200_1 = server
        .mock("GET", "/test1")
        .with_status(200)
        .with_body("success1")
        .expect(1)
        .create_async()
        .await;

    let url1 = format!("{}/test1", server.url());
    let result1 = rate_limited.get(&url1).await;
    assert!(result1.is_ok());

    mock_429_1.assert_async().await;
    mock_200_1.assert_async().await;

    // Second request: immediate success (backoff should have reset)
    let mock_200_2 = server
        .mock("GET", "/test2")
        .with_status(200)
        .with_body("success2")
        .expect(1)
        .create_async()
        .await;

    let url2 = format!("{}/test2", server.url());
    let start = Instant::now();
    let result2 = rate_limited.get(&url2).await;
    assert!(result2.is_ok());

    // Should be fast (no backoff delay for successful request)
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(50),
        "Should be fast without backoff: {:?}",
        elapsed
    );

    mock_200_2.assert_async().await;
}

#[tokio::test]
async fn test_different_platforms_have_separate_rate_limits() {
    // Modrinth has higher rate limits than CurseForge
    assert!(
        Platform::Modrinth.rate_limit() > Platform::CurseForge.rate_limit(),
        "Modrinth should have higher rate limit than CurseForge"
    );

    // Modrinth: 300/min, CurseForge: 60/min
    assert_eq!(Platform::Modrinth.rate_limit(), 300);
    assert_eq!(Platform::CurseForge.rate_limit(), 60);
}

#[tokio::test]
async fn test_backoff_config_validation() {
    let config = BackoffConfig::default();

    // Default values should be reasonable
    assert!(config.initial > Duration::from_millis(0), "Initial backoff should be positive");
    assert!(config.max > config.initial, "Max backoff should exceed initial");
    assert!(config.multiplier > 1.0, "Multiplier should be > 1 for exponential growth");
    assert!(config.multiplier <= 3.0, "Multiplier should be reasonable (not too aggressive)");
}
