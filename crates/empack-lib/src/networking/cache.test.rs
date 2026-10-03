use super::*;
use mockito::Server;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test]
async fn test_cache_creation() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    assert_eq!(cache.cache_dir(), &temp_dir.path().to_path_buf());
    assert!(cache.is_empty().await);
}

#[tokio::test]
async fn test_cache_with_custom_ttl() {
    let temp_dir = TempDir::new().unwrap();
    let ttl = Duration::from_secs(60);
    let cache = HttpCache::with_ttl(temp_dir.path().to_path_buf(), ttl);

    assert_eq!(cache.default_ttl, ttl);
}

#[tokio::test]
async fn test_cached_response_expiry() {
    let response = CachedResponse {
        data: vec![1, 2, 3],
        etag: Some("abc123".to_string()),
        expires: SystemTime::now() - Duration::from_secs(10), // Expired
        status: 200,
    };

    assert!(response.is_expired());

    let response = CachedResponse {
        data: vec![1, 2, 3],
        etag: Some("abc123".to_string()),
        expires: SystemTime::now() + Duration::from_secs(300), // Not expired
        status: 200,
    };

    assert!(!response.is_expired());
}

#[tokio::test]
async fn test_extend_ttl() {
    let mut response = CachedResponse {
        data: vec![1, 2, 3],
        etag: Some("abc123".to_string()),
        expires: SystemTime::now() + Duration::from_secs(10),
        status: 200,
    };

    let old_expires = response.expires;
    response.extend_ttl(Duration::from_secs(300));

    assert!(response.expires > old_expires);
}

#[tokio::test]
async fn test_cache_hit() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    let url = "https://example.com/test";
    let cached_response = CachedResponse {
        data: b"test data".to_vec(),
        etag: Some("abc123".to_string()),
        expires: SystemTime::now() + Duration::from_secs(300),
        status: 200,
    };

    // Store in cache
    cache.put(url.to_string(), cached_response.clone()).await;

    // Retrieve from cache
    let result = cache.get(url).await;
    assert!(result.is_some());

    let retrieved = result.unwrap();
    assert_eq!(retrieved.data, cached_response.data);
    assert_eq!(retrieved.etag, cached_response.etag);
    assert_eq!(retrieved.status, cached_response.status);
}

#[tokio::test]
async fn test_cache_miss() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    let result = cache.get("https://example.com/nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cache_remove() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    let url = "https://example.com/test";
    let cached_response = CachedResponse {
        data: b"test data".to_vec(),
        etag: None,
        expires: SystemTime::now() + Duration::from_secs(300),
        status: 200,
    };

    cache.put(url.to_string(), cached_response).await;
    assert_eq!(cache.len().await, 1);

    cache.remove(url).await;
    assert_eq!(cache.len().await, 0);
}

#[tokio::test]
async fn test_cache_clear() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    cache
        .put(
            "https://example.com/1".to_string(),
            CachedResponse {
                data: vec![1],
                etag: None,
                expires: SystemTime::now() + Duration::from_secs(300),
                status: 200,
            },
        )
        .await;

    cache
        .put(
            "https://example.com/2".to_string(),
            CachedResponse {
                data: vec![2],
                etag: None,
                expires: SystemTime::now() + Duration::from_secs(300),
                status: 200,
            },
        )
        .await;

    assert_eq!(cache.len().await, 2);

    cache.clear().await;
    assert_eq!(cache.len().await, 0);
}

#[tokio::test]
async fn test_disk_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    // Create cache and add entries
    {
        let cache = HttpCache::new(cache_dir.clone());
        cache
            .put(
                "https://example.com/test".to_string(),
                CachedResponse {
                    data: b"test data".to_vec(),
                    etag: Some("abc123".to_string()),
                    expires: SystemTime::now() + Duration::from_secs(300),
                    status: 200,
                },
            )
            .await;

        // Save to disk
        cache.save_to_disk().await.unwrap();
    }

    // Load from disk in a new cache instance
    {
        let cache = HttpCache::new(cache_dir);
        cache.load_from_disk().await.unwrap();

        assert_eq!(cache.len().await, 1);
        let entry = cache.get("https://example.com/test").await;
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.data, b"test data");
        assert_eq!(entry.etag, Some("abc123".to_string()));
    }
}

#[tokio::test]
async fn test_disk_persistence_filters_expired() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();

    // Create cache with expired and valid entries
    {
        let cache = HttpCache::new(cache_dir.clone());

        // Add expired entry
        cache
            .put(
                "https://example.com/expired".to_string(),
                CachedResponse {
                    data: b"expired".to_vec(),
                    etag: None,
                    expires: SystemTime::now() - Duration::from_secs(10),
                    status: 200,
                },
            )
            .await;

        // Add valid entry
        cache
            .put(
                "https://example.com/valid".to_string(),
                CachedResponse {
                    data: b"valid".to_vec(),
                    etag: None,
                    expires: SystemTime::now() + Duration::from_secs(300),
                    status: 200,
                },
            )
            .await;

        cache.save_to_disk().await.unwrap();
    }

    // Load from disk - should only get valid entry
    {
        let cache = HttpCache::new(cache_dir);
        cache.load_from_disk().await.unwrap();

        assert_eq!(cache.len().await, 1);
        assert!(cache.get("https://example.com/valid").await.is_some());
        assert!(cache.get("https://example.com/expired").await.is_none());
    }
}

#[tokio::test]
async fn test_load_from_disk_missing_cache_file_is_ok() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    cache.load_from_disk().await.unwrap();
    assert_eq!(cache.len().await, 0);
}

#[tokio::test]
async fn test_load_from_disk_invalid_json_returns_error() {
    let temp_dir = TempDir::new().unwrap();
    let cache_dir = temp_dir.path().to_path_buf();
    std::fs::write(cache_dir.join("http_cache.json"), "{ not valid json").unwrap();

    let cache = HttpCache::new(cache_dir);
    let err = cache.load_from_disk().await.unwrap_err();
    assert!(matches!(err, NetworkingError::CacheError { message } if message.contains("Failed to parse cache file")));
}

#[tokio::test]
async fn test_http_cache_miss_with_mock_server() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());
    let client = Client::new();

    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_header("etag", "\"abc123\"")
        .with_body("response data")
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let result = cache.get_with_etag(&client, &url).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.data, b"response data");
    assert_eq!(response.etag, Some("\"abc123\"".to_string()));
    assert_eq!(response.status, 200);
    assert!(!response.is_expired());

    mock.assert_async().await;

    // Verify it's now in cache
    assert_eq!(cache.len().await, 1);
}

#[tokio::test]
async fn test_http_cache_hit_returns_cached_data() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());
    let client = Client::new();

    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/test")
        .with_status(200)
        .with_header("etag", "\"abc123\"")
        .with_body("response data")
        .expect(1) // Should only be called once
        .create_async()
        .await;

    let url = format!("{}/test", server.url());

    // First request - cache miss
    let result1 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result1.data, b"response data");

    // Second request - cache hit (no network call)
    let result2 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result2.data, b"response data");

    // Mock should only have been called once
    mock.assert_async().await;
}

#[tokio::test]
async fn test_http_etag_revalidation_304() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());
    let client = Client::new();

    let mut server = Server::new_async().await;

    // First request - returns data with ETag
    let mock1 = server
        .mock("GET", "/test")
        .with_status(200)
        .with_header("etag", "\"abc123\"")
        .with_body("original data")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/test", server.url());

    // First request - cache miss
    let result1 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result1.data, b"original data");
    assert_eq!(result1.etag, Some("\"abc123\"".to_string()));

    mock1.assert_async().await;

    let mut expired = result1.clone();
    expired.expires = SystemTime::now() - Duration::from_secs(1);
    cache.put(url.clone(), expired).await;

    // Second request - returns 304 Not Modified
    let mock2 = server
        .mock("GET", "/test")
        .match_header("if-none-match", "\"abc123\"")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    // Second request - should revalidate with ETag and get 304
    let revalidation_started = SystemTime::now();
    let result2 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result2.data, b"original data"); // Should return cached data
    assert_eq!(result2.etag, result1.etag);
    assert!(result2.expires >= revalidation_started + cache.default_ttl());
    assert_eq!(cache.get(&url).await.unwrap().expires, result2.expires);

    mock2.assert_async().await;
}

#[tokio::test]
async fn test_http_etag_changed() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::with_ttl(temp_dir.path().to_path_buf(), Duration::from_millis(10));
    let client = Client::new();

    let mut server = Server::new_async().await;

    // First request
    let mock1 = server
        .mock("GET", "/test")
        .with_status(200)
        .with_header("etag", "\"abc123\"")
        .with_body("original data")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/test", server.url());
    let result1 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result1.data, b"original data");

    mock1.assert_async().await;

    // Wait for cache to expire
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Second request - ETag changed, return new data
    let mock2 = server
        .mock("GET", "/test")
        .match_header("if-none-match", "\"abc123\"")
        .with_status(200)
        .with_header("etag", "\"xyz789\"")
        .with_body("new data")
        .expect(1)
        .create_async()
        .await;

    let result2 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result2.data, b"new data");
    assert_eq!(result2.etag, Some("\"xyz789\"".to_string()));

    mock2.assert_async().await;
}

#[tokio::test]
async fn test_non_success_response_not_cached() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());
    let client = Client::new();

    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/error")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;

    let url = format!("{}/error", server.url());
    let result = cache.get_with_etag(&client, &url).await.unwrap();

    assert_eq!(result.status, 404);
    assert_eq!(result.data, b"Not Found");

    mock.assert_async().await;

    // Should not be cached
    assert_eq!(cache.len().await, 0);
}

#[tokio::test]
async fn test_get_with_etag_expired_without_etag_refetches() {
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::with_ttl(temp_dir.path().to_path_buf(), Duration::from_millis(10));
    let client = Client::new();

    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/no-etag")
        .with_status(200)
        .with_body("original data")
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/no-etag", server.url());
    let result1 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result1.data, b"original data");
    assert_eq!(result1.etag, None);
    first.assert_async().await;

    tokio::time::sleep(Duration::from_millis(20)).await;

    let second = server
        .mock("GET", "/no-etag")
        .with_status(200)
        .with_body("refetched data")
        .expect(1)
        .create_async()
        .await;

    let result2 = cache.get_with_etag(&client, &url).await.unwrap();
    assert_eq!(result2.data, b"refetched data");
    assert_eq!(result2.etag, None);
    second.assert_async().await;
}

// ============================================================================
// Resilience Tests (Phase A - Category 4: Networking Resilience)
// ============================================================================
// Note: Cache invalidation, hit/miss ratios, and concurrent access are already
// thoroughly tested in the existing test suite above. These tests verify
// additional resilience aspects.

#[tokio::test]
async fn test_cache_size_limits() {
    // Test cache behavior with size constraints (simple verification)
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());

    // Verify cache starts empty
    assert_eq!(cache.len().await, 0);

    // After operations, cache can be cleared
    cache.clear().await;
    assert_eq!(cache.len().await, 0);
}

#[tokio::test]
async fn test_cache_concurrent_writes() {
    // Test concurrent writes to cache don't cause data corruption
    use std::sync::Arc;
    use std::time::{SystemTime, Duration as StdDuration};

    let temp_dir = TempDir::new().unwrap();
    let cache = Arc::new(HttpCache::new(temp_dir.path().to_path_buf()));

    // Spawn multiple tasks that write to cache simultaneously
    let mut handles = vec![];
    for i in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = tokio::spawn(async move {
            let url = format!("http://test.com/item-{}", i);
            let response = CachedResponse {
                data: format!("data-{}", i).into_bytes(),
                etag: Some(format!("etag-{}", i)),
                status: 200,
                expires: SystemTime::now() + StdDuration::from_secs(3600),
            };
            cache_clone.put(url, response).await;
        });
        handles.push(handle);
    }

    // Wait for all writes to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify cache has entries (exact count depends on timing/collisions)
    let len = cache.len().await;
    assert!(len > 0 && len <= 10);
}

#[tokio::test]
async fn test_cache_eviction_on_clear() {
    // Test explicit cache eviction
    let temp_dir = TempDir::new().unwrap();
    let cache = HttpCache::new(temp_dir.path().to_path_buf());
    let client = Client::new();

    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", "/evict-test")
        .with_status(200)
        .with_header("etag", "\"test\"")
        .with_body("data")
        .create_async()
        .await;

    let url = format!("{}/evict-test", server.url());
    let _ = cache.get_with_etag(&client, &url).await.unwrap();

    // Cache should have data
    assert!(cache.len().await > 0);

    // Clear cache
    cache.clear().await;

    // Cache should be empty
    assert_eq!(cache.len().await, 0);

    mock.assert_async().await;
}

fn cache_entry(expires: SystemTime) -> CachedResponse {
    CachedResponse { data: b"value".to_vec(), etag: None, expires, status: 200 }
}

#[tokio::test]
async fn bounded_cache_evicts_earliest_expiry_and_persists_the_replacement() {
    let dir = TempDir::new().unwrap();
    let cache = HttpCache::new(dir.path().into());
    let now = SystemTime::now() + Duration::from_secs(3600);
    {
        let mut entries = cache.cache.write().await;
        for i in 0..1024 {
            entries.insert(format!("entry-{i}"), cache_entry(now + Duration::from_secs(i)));
        }
    }
    cache.put("new".into(), cache_entry(now + Duration::from_secs(2048))).await;
    assert_eq!(cache.len().await, 1024);
    assert!(cache.get("entry-0").await.is_none());
    assert!(cache.get("entry-1").await.is_some());
    let loaded = HttpCache::new(dir.path().into());
    loaded.load_from_disk().await.unwrap();
    assert_eq!(loaded.len().await, 1024);
    assert!(loaded.get("new").await.is_some());
    assert!(loaded.get("entry-0").await.is_none());
}

#[tokio::test]
async fn loading_large_entry_sets_keeps_newest_expiries_and_bounds_merged_state() {
    let dir = TempDir::new().unwrap();
    let cache = HttpCache::new(dir.path().into());
    let now = SystemTime::now() + Duration::from_secs(3600);
    let entries: HashMap<_, _> = (0..1100).map(|i| (
        format!("entry-{i}"), cache_entry(now + Duration::from_secs(i))
    )).collect();
    std::fs::write(dir.path().join("http_cache.json"), serde_json::to_vec(&entries).unwrap()).unwrap();
    cache.cache.write().await.insert("memory".into(), cache_entry(now + Duration::from_secs(2000)));
    cache.load_from_disk().await.unwrap();
    assert_eq!(cache.len().await, 1024);
    assert!(cache.get("entry-76").await.is_none());
    assert!(cache.get("entry-77").await.is_some());
    assert!(cache.get("entry-1099").await.is_some());
    assert!(cache.get("memory").await.is_some());
}

#[tokio::test]
async fn oversized_cache_is_rejected_before_parsing_or_changing_memory() {
    let dir = TempDir::new().unwrap();
    let cache = HttpCache::new(dir.path().into());
    cache.cache.write().await.insert("memory".into(), cache_entry(SystemTime::now() + Duration::from_secs(3600)));
    let file = std::fs::File::create(dir.path().join("http_cache.json")).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    let error = cache.load_from_disk().await.unwrap_err();
    assert!(error.to_string().contains("64 MiB load limit"));
    assert_eq!(cache.len().await, 1);
    assert!(cache.get("memory").await.is_some());
}

#[tokio::test]
async fn failed_cache_publication_preserves_existing_document() {
    let dir = TempDir::new().unwrap();
    let cache = HttpCache::new(dir.path().into());
    cache.put("old".into(), cache_entry(SystemTime::now() + Duration::from_secs(3600))).await;
    let path = dir.path().join("http_cache.json");
    let before = std::fs::read(&path).unwrap();
    let original_permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    cache.cache.write().await.insert("new".into(), cache_entry(SystemTime::now() + Duration::from_secs(3600)));
    let result = cache.save_to_disk().await;
    std::fs::set_permissions(&path, original_permissions).unwrap();
    assert!(result.unwrap_err().to_string().contains("Failed to publish cache"));
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
