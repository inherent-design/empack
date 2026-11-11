# Native engine bootstrap

Managed Prism and server distributions contain `.empack-tools/empack-bootstrap.jar` beside their launcher configuration or scripts, outside the game payload. This Java 8-compatible program selects an exact native empack engine and delegates an argument vector to it. Pack interpretation, acquisition, subscriptions, installation, runtime supervision and recovery remain in that engine. Standalone bundled snapshots and Modrinth exports do not require the bootstrap.

## Selection and compatibility

Generated commands request the exporter’s engine version. A distribution-local `empack` or `empack.exe` beside the JAR takes precedence when it reports that exact version. Otherwise the bootstrap uses its private user cache, then downloads the requested official release. It does not scan instances, follow a latest-version pointer, or upgrade a global installation. Preparation and launch request the same version independently of PATH or cache relocation.

Every delegated engine must report cancellation protocol `empack-bootstrap/1`; the first supporting engine is v0.8.5-beta. Display version and protocol capability are checked separately. A release descriptor's minimum engine requirement remains a separate compatibility check. A channel update cannot silently replace the engine selected by the launcher; rebuild and reimport the launcher distribution when intentionally changing that selection.

`--engine FILE` selects a strict local executable and fails on mismatch. `--development-engine` additionally permits an explicitly selected `0.0.0-dev` engine; it never admits that executable into a release cache slot. `--offline` prevents acquisition, while retaining matching distribution-local and cached engines. `--cache DIRECTORY` and `EMPACK_BOOTSTRAP_CACHE` select an isolated cache. Arguments after `--`, the working directory and inherited streams pass unchanged to native empack.

## Official acquisition and cache

The fixed official GitHub release API supplies the exact tag, archive name, byte count and SHA-256 digest. Verified HTTPS authenticates that authority; the digest is not an independent publisher signature. Downloads use the fixed official repository and known target naming, ignoring executable URLs in response data. Redirects are limited to GitHub’s configured asset hosts, with no HTTP downgrade, credentials, cookies or certificate bypass. Unavailable releases, missing digests and rate limits fail without choosing another version.

Requests have a shared 180-second deadline, five redirects, a 4 MiB metadata limit and a 64 MiB compressed archive limit. Archive inspection bounds decoded bytes and entry counts; it rejects traversal, aliases, duplicate entries, links, special files, unsupported compression, checksum failures and wrong native targets before execution. Target selection follows the JVM process architecture for the five shipped native targets. Unsupported architectures and exposed musl environments are refused.

The cache lives under `empack/engines` in the platform's user cache directory. Exact version spelling is hashed for a case-sensitive key even on a case-insensitive filesystem. A target-specific writer lock covers acquisition and publication, with a bounded wait; it is released before delegated execution. Private candidates and their ownership journal precede writing. Verified engine objects are immutable, and an atomic ready record publishes their identity. Interrupted candidates can be reclaimed; an admitted backing that is missing, corrupt, linked or writable outside the accepted user boundary fails visibly. Local engines are never moved or changed. Cache lookup records content and permissions, without persisting inode identities across reboots.

## Process lifetime

The JAR remains the native engine's parent. A nonce-authenticated loopback connection belongs to one invocation. Cancellation or loss of the Java parent feeds the existing native cancellation token. The native engine owns process retirement, terminal restoration and persisted runtime ownership; the bootstrap waits for its terminal result and preserves its exit status and diagnostics. It does not interpret a stopped process as retired or use Java `destroy()` as proof of runtime cleanup. The control descriptor is not passed to supervised game/tool processes.

Prism uses its selected `$INST_JAVA` for both preparation and wrapper execution. The wrapper leaves the original game Java program and JVM arguments in the delegated native argv; game flags are not applied to the bootstrap JVM. Server scripts use their existing `JAVA_HOME` or PATH Java selection. [Managed configuration validation](builds.md) and required server attributes apply to the resulting commands. Enrollment, signed-channel meaning, seed files, worlds and Prism icon ownership retain their ordinary engine checks.

## Build and qualification

The Java project remains in this repository. Its pinned modern JDK compiles with `--release 8`; packaged dependencies must retain that API and bytecode baseline. `python3 scripts/build-bootstrap.py` builds one shaded executable JAR and records its sources, version, hash and build tools. `--check` admits existing bytes without building or downloading. Cargo consumes this explicit input and refuses stale source or changed bytes; its build script does not build Java or use the network. Exporting with a packaged native empack therefore needs no JDK or bootstrap download.

CI builds the JAR once and tests those bytes across the admitted JVM majors. Native build evidence records the JAR identity emitted by Cargo. Release admission checks that all native archives embed the same JAR being published separately as `empack-bootstrap.jar`; nonpublishing candidate workflows retain their existing restrictions. Packaged-JAR fixtures, real terminal supervision and actual Prism import/preparation/wrapper checks establish different facts. A fixture pass does not establish a graphical launch or gameplay compatibility.
