# Feasibility gates and minimum browser profiles

The following targets define a useful minimum operating envelope for v0.9. They
are release requirements to test, not observed browser capabilities. They cannot be
lowered after a failing experiment without an explicit scope review. Native limits
remain separate; small browser processing limits do not cap retained asset size.

## Initial acceptance profiles

| Quantity | Desktop profile | Mobile profile |
| --- | --- | --- |
| Largest individual retained source | 1 GiB | 256 MiB |
| Aggregate custom-source fixture | 4 GiB | 1 GiB |
| Entries in tree/archive fixture | 20,000 | 5,000 |
| WASM linear-memory ceiling during fixture | 256 MiB | 128 MiB |
| Outstanding JS-owned processing buffers | 32 MiB | 16 MiB |
| Individual author/bindings document | 1 MiB | 256 KiB |
| Individual template input / output | 256 KiB / 4 MiB | 64 KiB / 1 MiB |
| Readable provider-response allowance | 4 MiB | 4 MiB |
| Metadata nesting / structural nodes per document | 64 / 100,000 | 64 / 25,000 |
| Maximum uninterrupted foreground compute step | 50 ms | 100 ms |
| Foreground cancel-message acknowledgement | 250 ms | 500 ms |
| Retirement of admitted local CPU/I/O work after cancel | 1 s | 2 s |

MiB/GiB are binary units. Text/response limits bound bytes, not unbounded aliases,
node expansion or template recursion. Responses and project metadata have their own
structural bounds established with fixtures. An operation with many small inputs
also meets its total admitted working-set bound; per-item bounds do not add up to
unlimited concurrent memory.

The source and entry tests are independent workload fixtures and a combined ordinary
pack fixture is required. Use deterministic incompressible bytes for large assets;
a highly compressible synthetic file is not the only large-input test. Stage and
retrieve an artifact representing the admitted aggregate inventory. Disk capacity
for source, private candidates and verified artifacts is budgeted explicitly; record
required scratch, retained bytes and available quota before the test. A browser's
storage estimate is not a reservation or guarantee.

Choose and record representative desktop, iOS and Android devices before measuring,
including model, RAM, OS/browser versions and power mode. Record warm/cold runs and
observed maxima, not only averages. JS-buffer accounting and WASM memory do not claim
total browser RSS. OS suspension and blocked user dialogs are reported separately
from foreground responsiveness. Network waiting has its own deadline; it cannot
keep a cancelled local compute task busy. Finalization uncertainty remains truthful.

## Early experiment

Before freezing the ports, demonstrate on actual required browser families:

```text
selected input → unique retained OPFS backing → bounded ZIP processing
    → task-queue yield → cancel acknowledgement and retirement
    → completed-run staged artifact → retrieved file
```

Start small to debug mechanics, then meet the profile above before certifying the
codec/storage boundary. Exercise stored and deflated members, many entries and
central-directory finalization. Include an admitted document parse, provider JSON
parse and template render in the responsiveness probe. A synchronous call wrapped
in a future is still one uninterrupted step.

Use a disposable experiment branch or harness module alongside checkpoint 1. Its
output is reproducible fixtures, measurements and a codec/adapter decision. Reuse
or replace it through the shared implementation in checkpoint 3; no duplicate browser
ZIP implementation remains. Feasibility evidence is not final release acceptance.

## Early mandatory network routes

Use actual browser Fetch from the intended secure-origin configuration for Modrinth
catalogs and a selected content/CDN route, plus uncached Minecraft/Fabric catalogs.
Test visible rate headers, redirects, readable bytes and integrity assertions. Check
client identification against provider policy and browser header controls. A 200
response or native CORS-header probe alone does not settle permitted integration.

Record how the browser request identifies the SDK where supported; resolve any
provider-policy ambiguity before claiming the route supported. Do not add a proxy,
shared secret or companion to make the experiment pass. An unavailable required route
blocks the corresponding release commitment until the design or scope is reviewed.

## Freeze gates

| Before freezing | Required evidence or contract |
| --- | --- |
| Draft/port APIs in #131 | Strict semantic-validation rule, proof ownership, sealing model, work classifications, initial feasibility findings |
| Native mutation boundary in #132 | Separate draft/project proofs and recoverable restore association |
| Format and ZIP in #133 | Wire fixtures including discovery-unknown, cooperative shared ZIP and thin real-browser adapter |
| Browser persistence in #134 | Non-nesting ownership, atomic ready-object registration, reader modes and failure tests |
| Export completion in #135 | Profile targets met by the shared path and truthful destination receipts |
| Release in #136 | Same-candidate native/desktop/mobile acceptance and fresh required-route checks |

Each expensive service is classified as admitted synchronous, resumable, or native
blocking work. Classification and input bounds are part of its API contract. An
experiment failing these targets calls for adaptation or an explicit design decision;
it must not silently produce a smaller claimed minimum after the fact.
