# Runtime inventory pin

`vanilla-1.21.1.json` records the official Minecraft 1.21.1 version metadata URL,
metadata SHA-1, server URL, byte count, original SHA-1 assertion, observed SHA-256
and Java major version. The metadata and streamed server bytes were checked against
the official assertions on 2026-10-26. Only identifiers and digests are checked in;
no Mojang runtime binary or source is included.

The live server-export fixture compares the ordinary executable's output with this
fixed inventory in ZIP, TAR.GZ and 7z, for referenced and bundled snapshots. Changing
a runtime pin requires separately checking the upstream bytes and reviewing the
new expectations. Do not regenerate the pin from the product's prepared release.
A fresh project store exercises acquisition; repeated formats in that fixture may
reuse verified cache content. This is not a claim that every build downloads anew.

The external harness streams large members to compute size and SHA-256, retains
only members up to 1 MiB for metadata/script inspection, and admits at most 256
entries and 128 MiB of compressed or decoded bytes. These are fixture bounds, not
product archive limits or a general hostile-archive validation contract. The test
checks consumer files and script attributes; real shell/batch execution and actual
Java startup have separate cases.
