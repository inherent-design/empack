# Archive header fixtures

`zip-headers.json` contains original synthetic bytes, maintained under the
repository license. It contains no downloaded pack, game, or third-party asset.
The records follow [PKWARE APPNOTE 6.3.10](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT),
sections 4.3.7, 4.3.12, and 4.3.14–4.3.16.

Both positive fixtures contain exactly one stored, empty regular file named
`empty`, with writable, non-executable Unix attributes. The ordinary fixture is
108 bytes; the ZIP64 fixture is 184 bytes. Its local record occupies bytes 0–34,
the central directory 35–85, the ZIP64 end record 86–141, the locator 142–161,
and the ordinary end record 162–183. The expected inventory is literal, not
derived from empack's encoder or planner.

Each negative record names a positive base and changes one field at a byte offset,
or truncates that base. Cases cover record signatures, disk declarations, directory
counts and bounds, variable-length fields, locator offsets, ZIP64 arithmetic,
entry limits, metadata limits, and short or overflowing end records.

The Rust driver applies the same bytes to metadata preflight, portable source
inspection, opaque inspection, and exact artifact verification. It also inserts a
ZIP64 extension block at the metadata-budget boundary without checking in a large
binary fixture. Separate namespace and corruption tests cover traversal, collisions,
links, CRC failures, and legal opaque JAR names. These native fixtures do not claim
browser codec execution or complete archive-format coverage.
