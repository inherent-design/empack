# empack

Empack manages Minecraft packs from one authored manifest and an exact resolution
lock, then prepares distributions for launchers and dedicated servers.

The target for this branch is **v0.6.0-beta**. Its [design contracts](docs/design/README.md)
define native installation and hosted updates, consumer-specific exports and
recoverable file publication. They describe the target behavior, not availability
in an earlier release. Source API documentation and executable help describe the
checked-out implementation. Implementation work belongs in [TODO.md](TODO.md).

## Installation

Download the archive for your operating system and architecture from
[GitHub Releases](https://github.com/inherent-design/empack/releases), extract it,
and place `empack` or `empack.exe` in a directory on your `PATH`.

```sh
empack --version
empack --help
```

Java is required for Minecraft and applicable loader installation. Empack itself
is a native executable. Release archives are executable distribution, not OS package
manager registrations.

## Pack authoring and distribution

Authors edit `empack.yml`; `empack.lock` records exact selections. Authoring operations
resolve dependencies and verify content before publication. Distribution recipes
select a consumer, dependency delivery and update authority. An installed instance
records its owned files and optional choices separately from the authoring project.

| Reference | Purpose |
| --- | --- |
| [Command contract](docs/usage.md) | Author and installed-instance workflows |
| [Consumer outputs](docs/design/builds.md) | Modrinth, CurseForge, Prism, server and native releases |
| [Instance updates](docs/design/instances.md) | File ownership, user edits, repair and rollback |
| [Release trust](docs/design/releases.md) | Immutable releases, subscriptions and publisher authentication |
| [Engine design](docs/design/architecture.md) | Interfaces, effects and runtime ownership |
| [Verification](docs/testing.md) | Execution and consumer acceptance |

## Contributing and license

See [CONTRIBUTING.md](CONTRIBUTING.md). Empack is licensed under
[Apache 2.0](LICENSE).
