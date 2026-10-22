# empack

Empack manages Minecraft modpacks from an editable `empack.yml`. It resolves exact
dependencies into `empack.lock`, builds launcher and server distributions, and can
install and update native pack releases while preserving player-owned files.

This documentation describes the **v0.8.3-beta development target**. It uses author schema 5 and optional
integration bindings schema 1. Older author documents require a new manifest or
re-import from a supported Modrinth or CurseForge archive; no schema converter is
provided. Use the documentation at your
installed release tag when running a downloaded binary.

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
compatible dependencies. `outdated` reports provider update evidence;
`migrate --minecraft VERSION --loader FAMILY --plan` previews a runtime change.
Commit `empack.yml`, `empack.lock` and any `empack.bindings.yml` with your sources.

For an existing pack archive, use `empack init my-pack --from ./pack.mrpack` or a
CurseForge manifest ZIP. Preview changes with `--dry-run`. See [usage](docs/usage.md)
for imports, optional files, restricted downloads and installed-instance commands.

## Pack authoring and distribution

Authors edit `empack.yml`; `empack.lock` records exact selections. Authoring operations
resolve dependencies and verify content before publication. Named build profiles
select a consumer, dependency delivery, update authority and format. Use
`empack build --explain` to inspect configured defaults before building. An installed instance records its owned files and optional choices separately from the authoring project.

The [manifest guide](docs/manifest.md) describes compact sources, pins and build profiles.
User guides cover [commands and workflows](docs/usage.md),
[running a server](docs/servers.md) and [publishing pack updates](docs/publishing.md).
The [documentation index](docs/README.md) separates these guides from development
references and engine contracts.

## Contributing and license

See [CONTRIBUTING.md](CONTRIBUTING.md). Empack is licensed under
[Apache 2.0](LICENSE).
