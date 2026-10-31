# Literal authoring fixture

These documents are authored inputs, not serialized product values.
`journey_authoring.rs` initializes a workspace, writes the manifest, reconciles it,
adds a locally generated metadata-only JAR, and applies the placement binding.
No Java bytecode or third-party mod is included.

The test checks complete member sets and independently specified bytes in bundled
Prism and Modrinth archives, including side-specific menu settings, root options,
OpenLoader-style paths, server properties, a rendered template, and excluded notes.
It edits options without changing the pack version, rebuilds, removes the dependency,
then rebuilds again to catch stale output. Source bytes and comments must survive.

This fixture does not establish game compatibility, actual launcher import, server
startup or reference preparation. Those require separate consumer/live variants.
Do not regenerate expected results by invoking the product planner or exporter.
