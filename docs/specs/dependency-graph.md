---
spec: dependency-graph
status: partial
created: 2026-04-11
updated: 2026-04-11
depends: [overview, types]
---

# Dependency Graph

The dependency graph API in `api/dependency_graph.rs` models dependency relationships between mod identifiers.

## Public Types

Current exported types:

- `DependencyGraph`
- `DependencyNode`
- `DependencyGraphError`

## Current Role

The graph is a library-level contract used to reason about transitive relationships and orphan detection.

Current guarantees:

- graph nodes are keyed by mod identifier
- missing-node access returns a typed `NodeNotFound` error
- graph construction and traversal are deterministic in-process behavior

## Wiring Status

This is partial rather than ratified because the API exists and is tested, but CLI-level orphan cleanup still uses it selectively rather than as the sole supported command path.
