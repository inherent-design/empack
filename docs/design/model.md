# Semantic values and documents

The model separates author requests, exact selections, portable releases and the
state of one installation.

## Identity

| Value | Meaning | Not interchangeable with |
| --- | --- | --- |
| `PackId` | Stable identity established when authoring begins | Display name, URL or version label |
| `DependencyKey` | Author-facing logical dependency label | Provider slug or metadata filename |
| `ProviderProjectId` | Provider-qualified canonical project | Search selector or exact file |
| `ResolvedPin` | Exact provider version/file selection | Follow-compatible policy |
| `FileRole` | Stable role within a resolved dependency | Filename |
| `ReleaseId` | Digest of exact portable release payload bytes | Human-readable release version |
| `ChoiceKey` | Stable optional decision identity | Position in a list |
| `InstallDestination` | Portable root-relative file placement | Host path or deletion grant |
| `ContentId` | Observed SHA-256 byte address | Authenticity of downloaded content |
| `InstanceId` | Native installation binding | Pack identity or author root |

Do not normalize provider case, Unicode spelling or filenames into a different
identity. A separate portable collision key rejects conflicting paths. Canonical
selectors are resolved before persistence; a slug never becomes an ID by assignment.

## Document roles

| Document | Owner | Contents |
| --- | --- | --- |
| `empack.yml` | Author | Pack/runtime intent, dependencies, source inclusion, consumer recipes |
| `empack.lock` | Resolver | Exact runtime and files, original assertions, placements, required closure evidence |
| Release manifest | Release builder | Portable exact inventory, runtime, choices, acquisition and installation policies |
| Channel envelope | Publisher | Pack/channel identity, sequence, expiry and authenticated release reference |
| Instance record | Instance engine | Last completed release, owned files, side, choices, authority and source binding |
| Recovery journal | Publisher | Approved operation, native root, before/after evidence and durable progress |

Each schema has its own explicit version. Unknown intent fields and invalid explicit
variants fail; they cannot silently become search input. Original author bytes are
preserved when no semantic edit is required. Canonical semantic identity and raw
file revision serve different purposes.

## Author intent and resolution

Dependency sources include provider selection, verified URL files, local files and
interpreted archive members. Mods, resource packs, shaders, datapacks and worlds
retain their content kind. Exact pins and follow-compatible selection are explicit.
Sync retains valid exact selections; update deliberately resolves eligible changes.
Unknown dependency coverage is not an empty required-dependency set.

A resolved dependency can contain multiple named file roles and placements. Source
path and destination are independent. Provider-owned world archives retain the
provider/exact archive identity and verified interpreted member ownership. A local
source change is observed drift, not automatic authorization to overwrite a project.

Source inclusion lives under authored layout policy in `empack.yml`. Rules are
captured with the read set and never inherited from host-global ignore files.
Reserved engine control files are excluded by the native layout adapter.

## Environments and optionality

Requirements retain client/server participation independently from optional choice.
Common, client and server layers have explicit precedence. Conditional replacement
and fallback relationships survive native releases. A consumer unable to represent
them requires selected choices or an explicit conversion; it cannot silently flatten
files or turn optional content into required content.

Choices have stable keys, allowed alternatives and defaults. Installed selections
are instance data. New constraints trigger a decision rather than resetting all
choices. Content disabled by a choice may retire only its unchanged owned files.

## Installed files

```rust
pub enum FilePolicy {
    Managed,
    Seed,
}

pub struct InstalledFile {
    pub baseline: FileContent,
    pub policy: FilePolicy,
}

pub struct ReleaseFile {
    pub content: FileContent,
    pub policy: FilePolicy,
}
```

`Managed` files participate in three-way reconciliation. `Seed` supplies initial
bytes only; existing bytes belong to the user. The instance record keeps the actual
installation baseline and records preserved seeds separately from incoming release
assertions. Worlds are seeded only by explicit initial installation and subsequently
excluded from ordinary update ownership. Runtime-generated files are user data.

## Integrity and provenance

Retain provider/source assertions alongside computed content IDs. MD5 compatibility
is weaker evidence; computing SHA-256 after downloading does not authenticate the
source. Signed release identity establishes publisher approval of exact descriptors,
not a new claim about the upstream author's signature or redistribution license.
