---
spec: networking-and-rate-budgets
status: draft
created: 2026-04-08
updated: 2026-04-08
depends: [overview]
---

# Networking and Rate Budgets

empack uses cached HTTP clients plus proactive rate pacing for Modrinth and CurseForge.

## Networking Components

| Component | Responsibility |
| --- | --- |
| `HttpCache` | Disk-backed response cache for supported request paths |
| `RateBudget` | Host budget interface |
| `HeaderDrivenBudget` | Modrinth budget driven by response headers |
| `FixedWindowBudget` | Conservative CurseForge budget with local window tracking |
| `HostBudgetRegistry` | Hostname to budget mapping |
| `RateLimitedClient` | Request execution with pacing and retry |
| `RateLimiterManager` | Modrinth and CurseForge client pair |
| `NetworkingManager` | Resource-aware concurrency manager for batch work |

## Budget Interface

`RateBudget` is synchronous.

Methods:

- `record_response(headers, status)`
- `acquire() -> Duration`
- `is_exhausted() -> bool`

The caller sleeps for the returned duration before sending the next request.

## Host Registry

`HostBudgetRegistry::new()` currently installs:

| Host | Budget |
| --- | --- |
| `api.modrinth.com` | `HeaderDrivenBudget::new(300)` |
| `api.curseforge.com` | `FixedWindowBudget::new(150, 60s)` |

Unknown hosts return no proactive budget.

## Modrinth Budget

`HeaderDrivenBudget` reads:

- `x-ratelimit-remaining`
- `x-ratelimit-limit`
- `x-ratelimit-reset`
- `retry-after` on `429`

Current pacing behavior is token-aware:

- high remaining budget returns no delay
- low remaining budget adds short sleeps
- exhausted budget waits until reset

This is the live behavior. The current code does not use an async budget trait.

## CurseForge Budget

`FixedWindowBudget` tracks request reservations in a time window.

Current behavior:

- normal requests reserve slots in the current window
- high usage adds a small pacing delay
- exhausted windows delay until the next window
- `403` forces exhaustion for the rest of the current window

This is intentionally conservative because CurseForge does not expose the same header feedback as Modrinth.

## Request Retry

`RateLimitedClient` composes proactive pacing with retry-on-429 behavior.

Current rules:

- apply `budget.acquire()` before each request when a budget exists
- record response headers back into the budget after the request
- retry `429` responses up to 5 times
- exponential backoff starts at 1 second and caps at 60 seconds by default
- reset backoff after a successful request

## Cache and Resolver Integration

The live network provider wires:

- one shared `reqwest::Client`
- one `HttpCache`
- one `RateLimiterManager`
- one `HostBudgetRegistry`

`ProjectResolver` receives those shared objects through `with_networking()`. Import resolution also uses the shared host budgets directly during manifest resolution.

## Resource-Aware Networking

`NetworkingManager` exists for batch concurrency and derives `optimal_jobs` from detected system resources.

Current configuration fields:

- `max_jobs`
- `timeout_seconds`
- `trace_requests`

This manager is part of the runtime library, but the live CLI still does not route every command path through it.
