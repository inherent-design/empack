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

The API remains available for graph analysis. `remove --deps` fails before mutation because installed packwiz metadata does not prove that dependency edges are complete. Manifest keys and provider IDs also use different namespaces. Automatic cleanup requires provider-qualified identities and complete reachability information before it can safely select removals. Explicit removal remains supported.
