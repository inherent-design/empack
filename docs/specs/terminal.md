---
spec: terminal
status: partial
created: 2026-04-11
updated: 2026-04-11
depends: [overview]
---

# Terminal

empack's terminal layer lives under `terminal/*.rs` and owns capability detection plus cursor recovery.

## Capabilities

`TerminalCapabilities` captures the current runtime terminal shape:

- color support
- unicode support
- TTY status
- width when available

`AppConfig.color` provides the detection intent: `auto`, `always`, or `never`.

## Safety and Recovery

Current runtime guarantees:

- the command loop forces cursor visibility before command execution
- a panic hook restores the cursor and cooperates with logger shutdown
- Ctrl+C handling restores the cursor, shuts down logging, removes the state marker when possible, and exits `130`

## Scope

This layer is a terminal-behavior contract, not a text-formatting contract. Exact wording belongs to the display and command layers.
