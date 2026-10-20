# Project automation

Generated GitHub workflows validate and publish the author’s selected distribution profiles.
The workflows and their Python helper are editable project files. Existing copies remain
user-owned; installing a newer empack executable does not replace them.

## Inputs and outputs

| Input | Default | Contract |
| --- | --- | --- |
| `EMPACK_VERSION` | Initializing release tag | Download the Linux x86-64 release executable and verify its GitHub asset SHA-256 digest before execution. Missing digest evidence fails installation. |
| `EMPACK_PROFILES` | Empty | Select `build.default`; otherwise select the named profiles, one per line. |
| `JAVA_VERSION` | `21` | Provision Temurin for server preparation. Authors select the Java version their Minecraft and loader require. |
| Optional files | Authored defaults | Pass `--optional-defaults` to explanation and execution. |
| Release tag | `v` plus authored version | Reject a mismatch before acquiring build inputs. |

Validation runs on main-branch pushes, pull requests and manual dispatch. It uploads
only the selected distributions and `SHA256SUMS` as a GitHub Actions artifact.
A pushed `v*` tag runs the same selection and build, then creates a GitHub release.
Tags containing a prerelease component produce prereleases. GitHub publication does
not upload to Modrinth or CurseForge or advance native empack subscription channels.

## Execution contract

The checked-in `.github/scripts/empack-ci.py` helper uses `build --explain --json`
to read schema-1 profile names, artifact paths, pack version and lock status. It requires
a current lock and binds execution to those exact profile names. Explanation verifies
configuration, not download availability. Build must complete before staging starts.
Restricted downloads or absent provider credentials fail the unattended job; workflows
never substitute another version or publish an incomplete pack.

Artifacts are copied from the exact explained paths into a new temporary staging
directory. Paths must remain within `dist/`, name regular files, and have no symlink
ancestors. Published names use ASCII letters, digits, dots, underscores and hyphens;
other characters become hyphens. Empty or colliding names fail. Checksums are computed
from staged bytes after naming, so each checksum names the actual downloadable asset.
Unselected files already in `dist/` are excluded.

Publication creates a draft with every staged asset, then makes it visible only after
upload succeeds. An existing release is not overwritten. Failed uploads may leave a
draft for the author to inspect and remove before retrying. Validation cancels obsolete
runs on the same ref; release runs serialize per tag without cancelling publication.

The helper downloads only the configured empack release from inherent-design/empack.
GitHub’s asset digest checks transfer integrity against GitHub metadata; it is not an
independent publisher signature. Actions use reviewed commit pins. The generated jobs
run on GitHub-hosted Ubuntu and require Python, GitHub CLI, network access and any pack
provider credentials. Release publication alone receives repository write permission.

## Templates

The default Prism configuration explicitly inherits launcher memory settings. Authors
who need pack-specific limits set `OverrideMemory=true`, `MinMemAlloc` and `MaxMemAlloc`
in `templates/client/instance.cfg.template`. Values are MiB. Empack does not infer a
safe heap from the mod count or reserve that heap through its own resource governor.

Server configuration remains editable under `templates/server/game/`. Whitelists,
world generators and pack-specific game rules belong in those files. Build recipes own
installer and launcher integration; templates must preserve required managed hooks.

## Verification

Tests execute the generated helper with deterministic release and CLI fixtures, then
exercise it against an initialized project using the actual empack executable. Cases
cover spaces in pack names, multiple profiles, a mismatched tag, changed download bytes,
missing digests, failed builds, stale outputs, path escapes, filename collisions and
partial upload failure. Workflow pins, permissions and helper wiring are checked
separately. A local fixture does not establish a successful hosted GitHub Actions run.
