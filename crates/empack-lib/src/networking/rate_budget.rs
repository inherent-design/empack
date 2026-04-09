use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Per-host rate budget tracking.
///
/// Two implementations: `HeaderDrivenBudget` (Modrinth) reads budget from
/// response headers. `FixedWindowBudget` (CurseForge) uses conservative
/// time-based limits with no header feedback.
///
/// `record_response` takes headers + status rather than the full `Response`
/// because the response body is consumed separately (e.g., by `json()`).
/// The caller extracts headers before consuming the body.
///
/// `acquire` returns the delay the caller should apply before sending the
/// next request.
pub trait RateBudget: Send + Sync {
    /// Record a response and update the budget from its headers.
    fn record_response(&self, headers: &HeaderMap, status: StatusCode);

    /// Calculate the delay required before making the next request.
    /// Returns zero when the request may proceed immediately.
    fn acquire(&self) -> Duration;

    /// Check if the budget is currently exhausted.
    fn is_exhausted(&self) -> bool;
}

// ---------------------------------------------------------------------------
// HeaderDrivenBudget (Modrinth)
// ---------------------------------------------------------------------------

/// Adaptive rate budget driven by `X-Ratelimit-*` response headers.
///
/// Reads `X-Ratelimit-Remaining`, `X-Ratelimit-Limit`, and
/// `X-Ratelimit-Reset` from every response to track the server-side
/// budget. When remaining tokens are low, `acquire()` introduces
/// progressive delays to avoid 429 responses.
pub struct HeaderDrivenBudget {
    remaining: AtomicU32,
    reset_at: AtomicU64,
    limit: AtomicU32,
}

impl HeaderDrivenBudget {
    const DEFAULT_429_RETRY_AFTER_SECS: u64 = 60;

    /// Create a new header-driven budget with the given initial limit.
    pub fn new(initial_limit: u32) -> Self {
        Self {
            remaining: AtomicU32::new(initial_limit),
            reset_at: AtomicU64::new(0),
            limit: AtomicU32::new(initial_limit),
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn parse_header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
        headers.get(name)?.to_str().ok()?.parse::<u32>().ok()
    }

    fn parse_header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
        headers.get(name)?.to_str().ok()?.parse::<u64>().ok()
    }
}

impl RateBudget for HeaderDrivenBudget {
    fn record_response(&self, headers: &HeaderMap, status: StatusCode) {
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.remaining.store(0, Ordering::Relaxed);
            let retry_after = Self::parse_header_u64(headers, "retry-after")
                .unwrap_or(Self::DEFAULT_429_RETRY_AFTER_SECS);
            let new_reset = Self::now_secs() + retry_after;
            self.reset_at.store(new_reset, Ordering::Relaxed);
            return;
        }

        if let Some(remaining) = Self::parse_header_u32(headers, "x-ratelimit-remaining") {
            self.remaining.store(remaining, Ordering::Relaxed);
        }
        if let Some(limit) = Self::parse_header_u32(headers, "x-ratelimit-limit") {
            self.limit.store(limit, Ordering::Relaxed);
        }
        if let Some(reset_secs) = Self::parse_header_u64(headers, "x-ratelimit-reset") {
            let new_reset = Self::now_secs() + reset_secs;
            self.reset_at.store(new_reset, Ordering::Relaxed);
        }
    }

    fn acquire(&self) -> Duration {
        loop {
            let mut remaining = self.remaining.load(Ordering::Relaxed);

            if remaining == 0 {
                let reset_at = self.reset_at.load(Ordering::Relaxed);
                let now = Self::now_secs();
                if reset_at > now {
                    return Duration::from_secs(reset_at - now);
                }

                let limit = self.limit.load(Ordering::Relaxed);
                if self
                    .remaining
                    .compare_exchange(0, limit, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
                {
                    continue;
                }
                remaining = limit;
            }

            let delay = match remaining {
                201.. => Duration::ZERO,
                101..=200 => Duration::ZERO,
                51..=100 => Duration::from_millis(50),
                21..=50 => Duration::from_millis(100),
                6..=20 => Duration::from_millis(500),
                1..=5 => Duration::from_millis(500),
                0 => continue,
            };

            if self
                .remaining
                .compare_exchange(
                    remaining,
                    remaining.saturating_sub(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return delay;
            }
        }
    }

    fn is_exhausted(&self) -> bool {
        self.remaining.load(Ordering::Relaxed) == 0
    }
}

// ---------------------------------------------------------------------------
// FixedWindowBudget (CurseForge)
// ---------------------------------------------------------------------------

/// Conservative fixed-window rate budget for APIs without header feedback.
///
/// Tracks requests in a sliding time window and blocks when the budget
/// is depleted. On 403 responses (CurseForge uses Cloudflare WAF),
/// forces exhaustion for the remainder of the current window
/// (up to `window_duration_secs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FixedWindowState {
    window_start: u64,
    reserved_in_window: u32,
}

pub struct FixedWindowBudget {
    state: Mutex<FixedWindowState>,
    max_per_window: u32,
    window_duration_secs: u64,
}

impl FixedWindowBudget {
    /// Create a new fixed-window budget.
    pub fn new(max_per_window: u32, window_duration: Duration) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            state: Mutex::new(FixedWindowState {
                window_start: now,
                reserved_in_window: 0,
            }),
            max_per_window,
            window_duration_secs: window_duration.as_secs(),
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn maybe_reset_window(state: &mut FixedWindowState, now: u64, window_duration_secs: u64) {
        if state.window_start <= now
            && now.saturating_sub(state.window_start) >= window_duration_secs
        {
            state.reserved_in_window = 0;
            state.window_start = now;
        }
    }
}

impl RateBudget for FixedWindowBudget {
    fn record_response(&self, _headers: &HeaderMap, status: StatusCode) {
        if status == StatusCode::FORBIDDEN {
            let now = Self::now_secs();
            let mut state = self.state.lock().expect("fixed window budget poisoned");
            Self::maybe_reset_window(&mut state, now, self.window_duration_secs);
            if state.window_start > now {
                // Future-window reservations already encode current-window
                // exhaustion. Rewinding would invalidate callers that already
                // reserved those future slots and are sleeping until that window.
                return;
            }
            state.reserved_in_window = self.max_per_window;
        }
    }

    fn acquire(&self) -> Duration {
        let now = Self::now_secs();
        let mut state = self.state.lock().expect("fixed window budget poisoned");
        Self::maybe_reset_window(&mut state, now, self.window_duration_secs);
        let threshold = (self.max_per_window as f64 * 0.8) as u32;

        if state.window_start > now {
            let delay = Duration::from_secs(state.window_start - now);
            if state.reserved_in_window >= self.max_per_window {
                state.window_start = state.window_start.saturating_add(self.window_duration_secs);
                state.reserved_in_window = 1;
                return Duration::from_secs(state.window_start - now);
            }

            let count = state.reserved_in_window;
            state.reserved_in_window = state.reserved_in_window.saturating_add(1);
            if count >= threshold {
                return delay.max(Duration::from_millis(100));
            }
            return delay;
        }

        if state.reserved_in_window >= self.max_per_window {
            state.window_start = state.window_start.saturating_add(self.window_duration_secs);
            state.reserved_in_window = 1;
            return Duration::from_secs(state.window_start.saturating_sub(now));
        }

        let count = state.reserved_in_window;
        state.reserved_in_window = state.reserved_in_window.saturating_add(1);
        if count >= threshold {
            return Duration::from_millis(100);
        }

        Duration::ZERO
    }

    fn is_exhausted(&self) -> bool {
        let now = Self::now_secs();
        let mut state = self.state.lock().expect("fixed window budget poisoned");
        Self::maybe_reset_window(&mut state, now, self.window_duration_secs);
        state.window_start > now || state.reserved_in_window >= self.max_per_window
    }
}

// ---------------------------------------------------------------------------
// NoOpBudget
// ---------------------------------------------------------------------------

/// No-op budget that never delays or blocks.
///
/// Used as the default when no host-specific budget is configured.
pub struct NoOpBudget;

impl RateBudget for NoOpBudget {
    fn record_response(&self, _headers: &HeaderMap, _status: StatusCode) {}

    fn acquire(&self) -> Duration {
        Duration::ZERO
    }

    fn is_exhausted(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// HostBudgetRegistry
// ---------------------------------------------------------------------------

/// Registry mapping API hostnames to their rate budgets.
///
/// Pre-populated with budgets for known platforms. Unknown hosts
/// return `None` from `for_url()`, meaning no proactive throttling.
pub struct HostBudgetRegistry {
    budgets: HashMap<String, Arc<dyn RateBudget>>,
}

impl HostBudgetRegistry {
    /// Create a registry pre-populated with known platform budgets.
    pub fn new() -> Self {
        let mut budgets: HashMap<String, Arc<dyn RateBudget>> = HashMap::new();
        budgets.insert(
            "api.modrinth.com".to_string(),
            Arc::new(HeaderDrivenBudget::new(300)),
        );
        budgets.insert(
            "api.curseforge.com".to_string(),
            Arc::new(FixedWindowBudget::new(150, Duration::from_secs(60))),
        );
        Self { budgets }
    }

    /// Create an empty registry (no budgets configured).
    pub fn empty() -> Self {
        Self {
            budgets: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_budgets(budgets: HashMap<String, Arc<dyn RateBudget>>) -> Self {
        Self { budgets }
    }

    /// Look up the rate budget for a URL by extracting its host.
    pub fn for_url(&self, url: &str) -> Option<Arc<dyn RateBudget>> {
        let host = extract_host(url)?;
        self.budgets.get(host).cloned()
    }

    /// Look up the rate budget for a known host string.
    pub fn for_host(&self, host: &str) -> Option<Arc<dyn RateBudget>> {
        self.budgets.get(host).cloned()
    }
}

impl Default for HostBudgetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = after_scheme.split('/').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    // -- extract_host -------------------------------------------------------

    #[test]
    fn extract_host_https() {
        assert_eq!(
            extract_host("https://api.modrinth.com/v2/search?q=foo"),
            Some("api.modrinth.com")
        );
    }

    #[test]
    fn extract_host_http() {
        assert_eq!(extract_host("http://example.com/path"), Some("example.com"));
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(
            extract_host("https://localhost:8080/path"),
            Some("localhost")
        );
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(extract_host("api.modrinth.com/v2"), None);
    }

    #[test]
    fn extract_host_empty() {
        assert_eq!(extract_host(""), None);
    }

    // -- HeaderDrivenBudget -------------------------------------------------

    #[test]
    fn header_budget_initial_state() {
        let budget = HeaderDrivenBudget::new(300);
        assert!(!budget.is_exhausted());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn header_budget_record_response_updates_remaining() {
        let budget = HeaderDrivenBudget::new(300);
        let headers = make_headers(&[
            ("x-ratelimit-remaining", "247"),
            ("x-ratelimit-limit", "300"),
            ("x-ratelimit-reset", "42"),
        ]);
        budget.record_response(&headers, StatusCode::OK);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 247);
        assert_eq!(budget.limit.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn header_budget_missing_headers_unchanged() {
        let budget = HeaderDrivenBudget::new(300);
        let empty = HeaderMap::new();
        budget.record_response(&empty, StatusCode::OK);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);
        assert_eq!(budget.limit.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn header_budget_malformed_headers_unchanged() {
        let budget = HeaderDrivenBudget::new(300);
        let headers = make_headers(&[
            ("x-ratelimit-remaining", "not-a-number"),
            ("x-ratelimit-limit", ""),
        ]);
        budget.record_response(&headers, StatusCode::OK);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);
        assert_eq!(budget.limit.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn header_budget_429_sets_exhausted() {
        let budget = HeaderDrivenBudget::new(300);
        let headers = make_headers(&[("retry-after", "5")]);
        budget.record_response(&headers, StatusCode::TOO_MANY_REQUESTS);
        assert!(budget.is_exhausted());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
        assert!(budget.reset_at.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn header_budget_429_without_retry_after() {
        let budget = HeaderDrivenBudget::new(300);
        let empty = HeaderMap::new();
        let before = HeaderDrivenBudget::now_secs();
        budget.record_response(&empty, StatusCode::TOO_MANY_REQUESTS);
        assert!(budget.is_exhausted());
        assert!(
            budget.reset_at.load(Ordering::Relaxed) >= before + 60,
            "429 without retry-after should still set a fallback reset window"
        );
    }

    #[test]
    fn header_budget_acquire_no_delay_high_remaining() {
        let budget = HeaderDrivenBudget::new(300);
        let delay = budget.acquire();
        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn header_budget_acquire_delay_low_remaining() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(50, Ordering::Relaxed);
        let delay = budget.acquire();
        assert_eq!(delay, Duration::from_millis(100));
    }

    #[test]
    fn header_budget_acquire_decrements_remaining() {
        let budget = HeaderDrivenBudget::new(300);
        budget.acquire();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 299);
    }

    #[test]
    fn header_budget_acquire_saturates_at_zero() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(1, Ordering::Relaxed);
        budget
            .reset_at
            .store(HeaderDrivenBudget::now_secs() + 60, Ordering::Relaxed);
        budget.acquire();
        budget.acquire();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn header_budget_acquire_waits_until_reset_when_exhausted() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(0, Ordering::Relaxed);
        budget
            .reset_at
            .store(HeaderDrivenBudget::now_secs() + 2, Ordering::Relaxed);

        let delay = budget.acquire();
        assert!(delay >= Duration::from_secs(1));
    }

    #[test]
    fn header_budget_acquire_refills_after_reset_passes() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(0, Ordering::Relaxed);
        budget.limit.store(300, Ordering::Relaxed);
        budget.reset_at.store(
            HeaderDrivenBudget::now_secs().saturating_sub(1),
            Ordering::Relaxed,
        );

        let delay = budget.acquire();
        assert_eq!(delay, Duration::ZERO);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 299);
    }

    // -- FixedWindowBudget --------------------------------------------------

    #[test]
    fn fixed_budget_initial_state() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn fixed_budget_record_increments() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        budget.acquire();
        assert_eq!(budget.state.lock().unwrap().reserved_in_window, 1);
    }

    #[test]
    fn fixed_budget_success_response_does_not_double_count() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        budget.acquire();
        budget.record_response(&HeaderMap::new(), StatusCode::OK);
        assert_eq!(budget.state.lock().unwrap().reserved_in_window, 1);
    }

    #[test]
    fn fixed_budget_403_forces_exhaustion() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        budget.record_response(&HeaderMap::new(), StatusCode::FORBIDDEN);
        assert!(budget.is_exhausted());
        assert_eq!(budget.state.lock().unwrap().reserved_in_window, 150);
    }

    #[test]
    fn fixed_budget_window_expiry_resets() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        *budget.state.lock().unwrap() = FixedWindowState {
            window_start: 0,
            reserved_in_window: 150,
        };
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn fixed_budget_acquire_no_delay_under_limit() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        let delay = budget.acquire();
        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn fixed_budget_acquire_delay_near_threshold() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(60));
        *budget.state.lock().unwrap() = FixedWindowState {
            window_start: FixedWindowBudget::now_secs(),
            reserved_in_window: 121,
        };
        let delay = budget.acquire();
        assert_eq!(delay, Duration::from_millis(100));
    }

    #[test]
    fn fixed_budget_acquire_preclaims_slots_before_response() {
        let budget = FixedWindowBudget::new(10, Duration::from_secs(60));

        for _ in 0..10 {
            let delay = budget.acquire();
            assert!(delay <= Duration::from_millis(100));
        }

        let delay = budget.acquire();
        assert!(delay >= Duration::from_secs(59));
    }

    #[test]
    fn fixed_budget_acquire_waits_when_window_exhausted() {
        let budget = FixedWindowBudget::new(150, Duration::from_secs(2));
        *budget.state.lock().unwrap() = FixedWindowState {
            window_start: FixedWindowBudget::now_secs(),
            reserved_in_window: 150,
        };

        let delay = budget.acquire();
        assert!(delay >= Duration::from_secs(1));
    }

    #[test]
    fn fixed_budget_403_exhaustion_delays_until_window_end() {
        let budget = FixedWindowBudget::new(10, Duration::from_secs(2));
        budget.record_response(&HeaderMap::new(), StatusCode::FORBIDDEN);

        let delay = budget.acquire();
        assert!(delay >= Duration::from_secs(1));
    }

    #[test]
    fn fixed_budget_403_preserves_future_window_reservations() {
        let budget = FixedWindowBudget::new(10, Duration::from_secs(60));
        let now = FixedWindowBudget::now_secs();
        *budget.state.lock().unwrap() = FixedWindowState {
            window_start: now + 60,
            reserved_in_window: 3,
        };

        budget.record_response(&HeaderMap::new(), StatusCode::FORBIDDEN);

        let state = *budget.state.lock().unwrap();
        assert_eq!(
            state,
            FixedWindowState {
                window_start: now + 60,
                reserved_in_window: 3,
            }
        );
    }

    #[test]
    fn fixed_budget_overflow_reserves_next_window_slot() {
        let budget = FixedWindowBudget::new(2, Duration::from_secs(60));
        let now = FixedWindowBudget::now_secs();
        *budget.state.lock().unwrap() = FixedWindowState {
            window_start: now,
            reserved_in_window: 2,
        };

        let delay = budget.acquire();
        let state = *budget.state.lock().unwrap();

        assert!(delay >= Duration::from_secs(59));
        assert_eq!(state.window_start, now + 60);
        assert_eq!(state.reserved_in_window, 1);
    }

    // -- NoOpBudget ---------------------------------------------------------

    #[test]
    fn noop_budget_never_delays() {
        let budget = NoOpBudget;
        let delay = budget.acquire();
        assert_eq!(delay, Duration::ZERO);
        assert!(!budget.is_exhausted());
    }

    // -- HostBudgetRegistry -------------------------------------------------

    #[test]
    fn registry_resolves_modrinth() {
        let reg = HostBudgetRegistry::new();
        assert!(
            reg.for_url("https://api.modrinth.com/v2/search?q=foo")
                .is_some()
        );
    }

    #[test]
    fn registry_resolves_curseforge() {
        let reg = HostBudgetRegistry::new();
        assert!(
            reg.for_url("https://api.curseforge.com/v1/mods/1234")
                .is_some()
        );
    }

    #[test]
    fn registry_unknown_host_returns_none() {
        let reg = HostBudgetRegistry::new();
        assert!(reg.for_url("https://example.com/foo").is_none());
    }

    #[test]
    fn registry_empty_has_no_budgets() {
        let reg = HostBudgetRegistry::empty();
        assert!(reg.for_url("https://api.modrinth.com/v2/search").is_none());
    }

    #[test]
    fn registry_for_host_direct() {
        let reg = HostBudgetRegistry::new();
        assert!(reg.for_host("api.modrinth.com").is_some());
        assert!(reg.for_host("unknown.example.com").is_none());
    }
}
