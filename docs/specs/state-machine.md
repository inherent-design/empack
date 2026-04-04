---
spec: state-machine
status: draft
created: 2026-04-04
updated: 2026-04-04
depends: [overview, types]
---

# Pack State Machine

The pack state machine governs project lifecycle transitions. State is discovered from filesystem presence, not stored explicitly.

## States

| State | Discovery criteria |
|-------|-------------------|
| Uninitialized | No empack.yml or pack/ directory |
| Configured | empack.yml exists; pack/ may be initialized |
| Built | Build artifacts exist in dist/ |
| Building | Transient; active build in progress |
| Cleaning | Transient; active clean in progress |
| Interrupted { was } | A Building or Cleaning operation was interrupted |

## Transitions

```
Uninitialized -> Configured    (Initialize)
Configured -> Configured       (RefreshIndex)
Configured -> Built            (Build)
Built -> Configured            (Clean)
```

### Initialize

Input: `InitializationConfig` (name, author, version, modloader, mc_version, loader_version).

Effects:
1. Create empack.yml
2. Create pack/ directory
3. Run `packwiz init` with parameters
4. Scaffold template files (.gitignore, .packwizignore, templates/, dist/, .github/workflows/)

Properties:
- Fails if directory already contains empack.yml (unless `--force`).
- On packwiz failure, empack.yml is cleaned up (error recovery).
- Template scaffolding runs after state transition to avoid `discover_state` seeing dist/ as Built.

### Build

Input: `BuildOrchestrator`, `Vec<BuildTarget>`.

Effects per target:
- Mrpack: `packwiz refresh` then `packwiz mr export`
- Server: template rendering, Java server installer invocation
- Client: packwiz-installer for client directory layout
- Full variants: combine server/client with archive packaging

### Clean

Effects: remove dist/ contents. Transition from Built back to Configured.

## empack.yml Schema

```yaml
empack:
  name: "Pack Name"
  author: "Author"
  version: "1.0.0"
  minecraft_version: "1.21.1"
  loader: fabric
  dependencies:
    - sodium:
        status: resolved
        title: Sodium
        platform: modrinth
        project_id: AANobbMI
        project_type: mod
```

Properties:
- `dependencies` is an array of single-key maps. The key is a kebab-case identifier.
- Each dependency carries `status`, `title`, `platform`, `project_id`, `project_type`, and optionally `version`.
- `empack sync` reconciles this list against packwiz's installed .pw.toml files.
