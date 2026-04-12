---
spec: session-security
status: partial
created: 2026-04-11
updated: 2026-04-11
depends: [overview, session-providers]
---

# Session Security

This spec documents the security-sensitive filesystem and process boundaries around live sessions.

## Filesystem Provider Posture

`LiveFileSystemProvider` is path-transparent. It is not a sandbox and does not claim traversal isolation on its own.

Current security posture:

- path trust comes from command/workflow validation, not provider-level confinement
- tracked local dependency paths are stored project-relative in `empack.yml`
- commands resolve those project-relative paths against the active workdir before file operations

## Process and Interrupt Boundaries

Current live-session behavior includes:

- managed or overridden `packwiz-tx` execution through the process provider
- best-effort state-marker cleanup on Ctrl+C, bounded to the active project
- cursor restoration and logger shutdown on panic or interrupt

Subprocess interrupt cleanup is allowed to walk upward from the subprocess working directory, but it stops once the marker is removed or the nearest empack project boundary is reached. It does not continue into parent projects.

## Contract Boundary

This is a partial spec because the runtime behavior is real and tested, but the threat model is still workflow-oriented rather than a hardened sandbox contract.
