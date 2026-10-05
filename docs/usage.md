# CLI migration target

The release target is v0.5.0-alpha.1. Existing commands remain available through
compatibility adapters while operations move into the typed engine.

| Current command | Target request |
| --- | --- |
| `init` | Initialize |
| `init --from` | Import |
| `add` | Add |
| `remove` | Remove |
| `sync` | Sync against exact resolution |
| `build` | Build from satisfied intent or explicit observed-snapshot policy |
| `clean` | Managed project cleanup and separate host-cache maintenance |
| `version`, `requirements` | Host inspection without project mutation |

`Update`, `AdoptObserved` and `Migrate` are target requests. Their CLI spellings
are not available until implemented and covered in the parity ledger. The same
applies to the future public `Engine` API; illustrative design calls are not
current executable examples.

The [baseline command reference](compatibility/usage-0.4.md) preserves existing
flags, environment precedence, supported formats and exit classes. The target
[engine API](design/api.md) specifies the replacement orchestration. Preview must
have read-only durable storage capabilities; legacy dry-run behavior is not proof
that every command already satisfies that target.

See [policy decisions](design/decisions.md), [feature parity](design/parity.md) and
[implementation status](design/implementation.md) before relying on new behavior.
