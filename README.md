# empack

Empack manages Minecraft modpacks from one editable `empack.yml`. It resolves exact
dependencies into `empack.lock`, builds launcher and server distributions, and can
install and update native pack releases while preserving player-owned files.

This documentation covers **v0.6.0-beta**. It replaces the v0.5 authoring schema and
packwiz integration; existing projects require a new manifest or re-import from a
supported Modrinth or CurseForge archive. There is no automatic schema migration.

## Installation

Download the archive for your operating system and architecture from
[GitHub Releases](https://github.com/inherent-design/empack/releases), extract it,
and place `empack` or `empack.exe` in a directory on your `PATH`.

```sh
empack --version
empack --help
```

Choose the beta release explicitly; GitHub's “latest” link may point to a different
release. Empack is a native executable and does not require packwiz, its installer
JARs, or a system `7z` command. Java is needed for Minecraft and applicable server
loader installation. Client launchers manage their own Java and game runtime.

## Quickstart

Create a Fabric pack, add a mod, and export it for a launcher. These commands need
network access; initialization prompts for any missing project settings.

```sh
empack init my-pack --modloader fabric --mc-version 1.21.1
empack --workdir my-pack add --platform modrinth sodium
empack --workdir my-pack build modrinth
```

Import the `.mrpack` from `my-pack/dist/` into a compatible launcher such as Prism.
Add authored configuration under `my-pack/pack/`. After editing `empack.yml`, run
`empack --workdir my-pack sync`; use `update` when you deliberately want newer
compatible dependencies. Commit both `empack.yml` and `empack.lock` with your sources.

For an existing pack archive, use `empack init my-pack --from ./pack.mrpack` or a
CurseForge manifest ZIP. Preview changes with `--dry-run`. See [usage](docs/usage.md)
for imports, optional files, restricted downloads and installed-instance commands.

## Pack authoring and distribution

Authors edit `empack.yml`; `empack.lock` records exact selections. Authoring operations
resolve dependencies and verify content before publication. Distribution recipes
select a consumer, dependency delivery and update authority. An installed instance
records its owned files and optional choices separately from the authoring project.

| Reference | Purpose |
| --- | --- |
| [Usage](docs/usage.md) | Author and installed-instance workflows |
| [Consumer outputs](docs/design/builds.md) | Modrinth, CurseForge, Prism, server and native releases |
| [Instance updates](docs/design/instances.md) | File ownership, user edits, repair and rollback |
| [Publisher setup](docs/publishing.md) | Signing keys, static hosting and channel publication |
| [Release trust](docs/design/releases.md) | Immutable releases, subscriptions and publisher authentication |
| [Engine design](docs/design/architecture.md) | Interfaces, effects and runtime ownership |
| [Verification](docs/testing.md) | Execution and consumer acceptance |

## Contributing and license

See [CONTRIBUTING.md](CONTRIBUTING.md). Empack is licensed under
[Apache 2.0](LICENSE).
