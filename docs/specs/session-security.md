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
- preservation of operation markers on cancellation
- cursor restoration and logger shutdown on panic or interrupt

Subprocess cancellation leaves project markers intact. Only a completed operation or successful explicit recovery removes them.

## Contract Boundary

This is a partial spec because the runtime behavior is real and tested, but the threat model is still workflow-oriented rather than a hardened sandbox contract.

## Generated Destinations

Artifact names and versions must be portable filename components. Build and target-clean operations validate output paths before creating or deleting artifacts. The live provider rejects symlinks beneath the selected project root, including a symlinked `dist/` tree. These checks do not provide isolation against another process replacing a path after validation.

Restricted continuation accepts destinations only under selected full-build directories or the configured packwiz import cache. Cache locations must match runtime configuration, filenames must be single components, and live destination checks reject symlinked ancestors. Export passes an explicit managed cache path to packwiz, partitioned by project identity so equal filenames from different projects cannot replace each other. Continuation files created with a different cache location require a fresh build; project fingerprints alone do not authorize paths.
