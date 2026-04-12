---
spec: display
status: partial
created: 2026-04-11
updated: 2026-04-11
depends: [overview, terminal]
---

# Display

empack's display layer is the user-facing status surface built on `display/*.rs`.

## Responsibilities

The display contract currently covers:

- status lines such as checking, success, warning, error, and complete
- lightweight sections and subtle follow-up text
- progress and issue streaming for long-running subprocess work
- structured rows and tables for richer command output

## Runtime Wiring

The live command path uses `LiveDisplayProvider` and the shared display singleton initialized during session construction.

Current boundaries:

- commands write user-facing progress through `Session::display()`
- streaming subprocess issues are forwarded through `IssueStreamObserver`
- unit tests validate formatting helpers and command intent, but they do not capture the exact same live terminal rendering path as subprocess E2E

## Stability Notes

This is a partial spec because the runtime surface exists and is tested, but the full output taxonomy is not yet frozen line-by-line.

Stable expectations today:

- command failures must surface an error state before returning a non-zero process exit
- command summaries use status sections rather than raw `println!`
- display behavior must remain compatible with `NO_COLOR=1` and non-interactive terminals
