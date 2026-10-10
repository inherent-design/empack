# Product decisions

Empack owns authoring and native installed-instance management. Consumers receive
explicit projections of the same exact resolution.

| Decision | Contract |
| --- | --- |
| Release | `v0.6.0-beta`; development package versions remain tag-derived |
| Authoring | One editable `empack.yml` and generated exact `empack.lock` |
| Identity | Provider project, exact file, logical role, destination and ownership are distinct |
| Foreign backend | No packwiz observation, metadata maintenance, import/export or JAR runtime |
| Consumers | Modrinth, CurseForge, Prism, dedicated server and native empack |
| Delivery | Download on installation or bundled pack content, where the consumer permits it |
| Default updates | Fixed snapshot; subscriptions require explicit selection |
| Update authority | Exactly one of snapshot, platform or empack for an installation |
| Hosted updates | Static HTTPS channel pointing to immutable authenticated releases |
| Unattended trust | Signed channel and release metadata with explicitly enrolled publisher keys |
| Tool updates | Separate operator policy; pack metadata cannot replace the empack executable |
| Default batch | All requested items verify before publication |
| Partial progress | Explicit independent author dependency groups only; never partial instance activation |
| Source evidence | Provider compatibility hashes retain their strength classification |
| User data | Unowned files, modified configuration and played worlds are protected |
| Recovery | Durable file-level publication with expected-old checks; no global atomic visibility claim |
| Runtime | One host Tokio runtime and engine-owned task retirement |
| Program delivery | Native binaries and thin launcher integration; no second installer engine |
| Schema support | Strict versioned documents; no automatic legacy-schema migration |

## Scope boundaries

Keep provider and direct/local content, supported content kinds, imports, exact pins,
side layers, optional groups, named placements, manual acquisition, archives,
templates and runtime families. Removing an obsolete backend does not authorize
removing these capabilities.

Marketplace-compatible exports are in scope. Marketplace accounts and upload
automation, a standalone game launcher, OS package manager integrations, a custom
hosting service and a background update daemon are outside this release contract.

The engine does not execute arbitrary scripts from downloaded release metadata.
Trusted author templates remain explicit build inputs. External loader installers
require their own verified assets and execution grant.

Snapshot installation may accept a directly approved release digest without a
channel subscription. Unattended channel updates require authenticated publisher
identity, expiry and replay checks. A digest fetched beside untrusted bytes alone
is not publisher authentication.
