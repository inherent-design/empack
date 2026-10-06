use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

    /// A change invalidates outstanding delayed reservations. Callers must re-acquire.
    fn generation(&self) -> u64 {
        0
    }

    /// Check if the budget is currently exhausted.
    fn is_exhausted(&self) -> bool;
}

/// Wait for a reservation, replacing it if response feedback invalidates its window.
pub async fn wait_for_budget(budget: &dyn RateBudget) {
    loop {
        let generation = budget.generation();
        let delay = budget.acquire();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if generation == budget.generation() {
            return;
        }
    }
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
    clock_origin: Instant,
    remaining: AtomicU32,
    reset_at: AtomicU64,
    limit: AtomicU32,
    reservations: Mutex<(u64, u32)>,
    observed_reset: AtomicBool,
    generation: AtomicU64,
}

impl HeaderDrivenBudget {
    const DEFAULT_429_RETRY_AFTER_SECS: u64 = 60;
    const SECOND: u64 = 1_000_000_000;
    const WINDOW: u64 = 60 * Self::SECOND;

    /// Create a new header-driven budget with the given initial limit.
    pub fn new(initial_limit: u32) -> Self {
        Self {
            clock_origin: Instant::now(),
            remaining: AtomicU32::new(initial_limit.max(1)),
            reset_at: AtomicU64::new(Self::WINDOW),
            limit: AtomicU32::new(initial_limit.max(1)),
            reservations: Mutex::new((0, 0)),
            observed_reset: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    // Relative monotonic nanoseconds preserve the full provider cooldown, even when a
    // response arrives immediately before a wall-clock second boundary.
    fn now_ticks(&self) -> u64 {
        self.clock_origin
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    fn acquire_at(&self, now: u64) -> Duration {
        let mut reservations = self.reservations.lock().expect("header budget poisoned");
        let limit = self.limit.load(Ordering::Relaxed).max(1);
        let reset = self.reset_at.load(Ordering::Relaxed);
        if reservations.0 != 0 && reservations.0 <= now {
            self.remaining
                .store(limit.saturating_sub(reservations.1), Ordering::Relaxed);
            self.reset_at.store(
                reservations.0.saturating_add(Self::WINDOW),
                Ordering::Relaxed,
            );
            *reservations = (0, 0);
        } else if reset <= now && reservations.0 == 0 {
            self.remaining.store(limit, Ordering::Relaxed);
            self.reset_at
                .store(now.saturating_add(Self::WINDOW), Ordering::Relaxed);
        }
        let remaining = self.remaining.load(Ordering::Relaxed);
        if remaining == 0 || reservations.0 > now {
            let next = self.reset_at.load(Ordering::Relaxed).max(now);
            if reservations.0 < next {
                *reservations = (next, 0);
            }
            if reservations.1 >= limit {
                reservations.0 = reservations.0.saturating_add(Self::WINDOW);
                reservations.1 = 0;
            }
            reservations.1 += 1;
            return Duration::from_nanos(reservations.0.saturating_sub(now));
        }
        self.remaining.store(remaining - 1, Ordering::Relaxed);
        match remaining {
            101.. => Duration::ZERO,
            51..=100 => Duration::from_millis(50),
            21..=50 => Duration::from_millis(100),
            _ => Duration::from_millis(500),
        }
    }

    fn parse_header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
        headers.get(name)?.to_str().ok()?.parse::<u32>().ok()
    }

    fn parse_header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
        headers.get(name)?.to_str().ok()?.parse::<u64>().ok()
    }
}

impl HeaderDrivenBudget {
    fn record_response_at(&self, headers: &HeaderMap, status: StatusCode, now: u64) {
        let mut reservations = self.reservations.lock().expect("header budget poisoned");
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.remaining.store(0, Ordering::Relaxed);
            let retry_after = Self::parse_header_u64(headers, "retry-after")
                .unwrap_or(Self::DEFAULT_429_RETRY_AFTER_SECS);
            let new_reset = now.saturating_add(retry_after.saturating_mul(Self::SECOND));
            if self.observed_reset.swap(true, Ordering::Relaxed) {
                self.reset_at.fetch_max(new_reset, Ordering::Relaxed);
            } else {
                self.reset_at.store(new_reset, Ordering::Relaxed);
            }
            *reservations = (0, 0);
            self.generation.fetch_add(1, Ordering::SeqCst);
            return;
        }

        if let Some(remaining) = Self::parse_header_u32(headers, "x-ratelimit-remaining") {
            self.remaining.fetch_min(remaining, Ordering::Relaxed);
        }
        if let Some(limit) = Self::parse_header_u32(headers, "x-ratelimit-limit") {
            self.limit.store(limit.max(1), Ordering::Relaxed);
            self.remaining.fetch_min(limit.max(1), Ordering::Relaxed);
        }
        if let Some(reset_secs) = Self::parse_header_u64(headers, "x-ratelimit-reset") {
            let new_reset = now.saturating_add(reset_secs.saturating_mul(Self::SECOND));
            let previous = self.reset_at.load(Ordering::Relaxed);
            if self.observed_reset.swap(true, Ordering::Relaxed) {
                self.reset_at.fetch_max(new_reset, Ordering::Relaxed);
            } else {
                self.reset_at.store(new_reset, Ordering::Relaxed);
            }
            if new_reset > previous {
                *reservations = (0, 0);
                self.generation.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
impl RateBudget for HeaderDrivenBudget {
    fn record_response(&self, headers: &HeaderMap, status: StatusCode) {
        self.record_response_at(headers, status, self.now_ticks());
    }
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn acquire(&self) -> Duration {
        self.acquire_at(self.now_ticks())
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
        let before = budget.now_ticks();
        budget.record_response(&empty, StatusCode::TOO_MANY_REQUESTS);
        assert!(budget.is_exhausted());
        assert!(
            budget.reset_at.load(Ordering::Relaxed) >= before + HeaderDrivenBudget::WINDOW,
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
        budget.reset_at.store(
            budget.now_ticks() + HeaderDrivenBudget::WINDOW,
            Ordering::Relaxed,
        );
        budget.acquire();
        budget.acquire();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn header_budget_acquire_waits_until_reset_when_exhausted() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(0, Ordering::Relaxed);
        budget.reset_at.store(
            budget.now_ticks() + 2 * HeaderDrivenBudget::SECOND,
            Ordering::Relaxed,
        );

        let delay = budget.acquire();
        assert!(delay >= Duration::from_secs(1));
    }

    #[test]
    fn header_budget_acquire_refills_after_reset_passes() {
        let budget = HeaderDrivenBudget::new(300);
        budget.remaining.store(0, Ordering::Relaxed);
        budget.limit.store(300, Ordering::Relaxed);
        budget
            .reset_at
            .store(budget.now_ticks().saturating_sub(1), Ordering::Relaxed);

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
    #[test]
    fn provider_cooldown_retains_fractional_second_at_response_boundary() {
        let budget = HeaderDrivenBudget::new(300);
        let second = HeaderDrivenBudget::SECOND;
        budget.record_response_at(
            &make_headers(&[("x-ratelimit-remaining", "5"), ("x-ratelimit-reset", "1")]),
            StatusCode::OK,
            second - 1,
        );
        assert_eq!(budget.acquire_at(second), Duration::from_millis(500));
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 4);
        assert_eq!(budget.reset_at.load(Ordering::Relaxed), 2 * second - 1);
        budget.record_response_at(
            &make_headers(&[("retry-after", "1")]),
            StatusCode::TOO_MANY_REQUESTS,
            2 * second - 1,
        );
        assert_eq!(
            budget.acquire_at(2 * second),
            Duration::from_nanos(second - 1)
        );
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn exhausted_header_budget_reserves_distinct_future_windows() {
        let budget = HeaderDrivenBudget::new(2);
        budget.remaining.store(0, Ordering::Relaxed);
        budget
            .reset_at
            .store(110 * HeaderDrivenBudget::SECOND, Ordering::Relaxed);
        let delays: Vec<_> = (0..5)
            .map(|_| {
                budget
                    .acquire_at(100 * HeaderDrivenBudget::SECOND)
                    .as_secs()
            })
            .collect();
        assert_eq!(delays, [10, 10, 70, 70, 130]);
    }

    #[test]
    fn stale_headers_cannot_restore_reserved_tokens_or_shorten_reset() {
        let budget = HeaderDrivenBudget::new(300);
        budget.record_response(
            &make_headers(&[("x-ratelimit-remaining", "4"), ("x-ratelimit-reset", "60")]),
            StatusCode::OK,
        );
        budget.acquire();
        let reset = budget.reset_at.load(Ordering::Relaxed);
        budget.record_response(
            &make_headers(&[("x-ratelimit-remaining", "20"), ("x-ratelimit-reset", "5")]),
            StatusCode::OK,
        );
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 3);
        assert!(budget.reset_at.load(Ordering::Relaxed) >= reset);
    }
    #[test]
    fn later_cooldown_cannot_be_overwritten_by_a_reserved_window() {
        let budget = HeaderDrivenBudget::new(2);
        let now = budget.now_ticks();
        budget.remaining.store(0, Ordering::Relaxed);
        budget
            .reset_at
            .store(now + 10 * HeaderDrivenBudget::SECOND, Ordering::Relaxed);
        assert_eq!(budget.acquire_at(now).as_secs(), 10);
        budget.record_response(
            &make_headers(&[("retry-after", "120")]),
            StatusCode::TOO_MANY_REQUESTS,
        );
        assert!(
            budget
                .acquire_at(now + 10 * HeaderDrivenBudget::SECOND)
                .as_secs()
                >= 110
        );
        assert!(budget.reset_at.load(Ordering::Relaxed) >= now + 120 * HeaderDrivenBudget::SECOND);
    }
    #[tokio::test]
    async fn waiting_request_reacquires_an_invalidated_reservation() {
        struct InvalidatedBudget {
            generation: AtomicU64,
            calls: AtomicU32,
        }
        impl RateBudget for InvalidatedBudget {
            fn record_response(&self, _: &HeaderMap, _: StatusCode) {}
            fn is_exhausted(&self) -> bool {
                false
            }
            fn generation(&self) -> u64 {
                self.generation.load(Ordering::SeqCst)
            }
            fn acquire(&self) -> Duration {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.generation.fetch_add(1, Ordering::SeqCst);
                    Duration::from_millis(1)
                } else {
                    Duration::ZERO
                }
            }
        }
        let budget = InvalidatedBudget {
            generation: AtomicU64::new(0),
            calls: AtomicU32::new(0),
        };
        wait_for_budget(&budget).await;
        assert_eq!(budget.calls.load(Ordering::SeqCst), 2);
    }
}
