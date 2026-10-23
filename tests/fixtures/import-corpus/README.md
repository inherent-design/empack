# Pinned import corpus

These original synthetic archives are distributed under the repository's
[Apache-2.0 license](../../../LICENSE). They contain no third-party mods or assets.
`manifest.json` pins their SHA-256 identities and independently authored expected
Modrinth output members. ZIP timestamps and Unix file attributes are fixed.

The corpus covers Modrinth side-specific overrides, CurseForge common overrides,
exact runtime selections, and traversal refusal. `journey_import` checks the hashes
before invoking the ordinary executable, preserves imported documents across two
syncs, and compares the complete exported content inventory and runtime metadata.
The fixture expectation is not derived from empack's importer or writer.

Run `python3 scripts/check-import-corpus.py` from the repository root. It uses the
shared verification runner, including executable identity and result receipts.
This local corpus needs no network, Java, or provider credentials. It does not
qualify external provider behavior or real launcher/game consumption.

Changes to archives or expected output require deliberate review of both the
source contents and expectations. The checker never refreshes them. Moving
upstream packs belong to `scripts/survey-imports.py`; survey observations cannot
replace these pinned expectations.
