#[path = "support/local_exports.rs"]
mod local_exports;

#[test]
fn journey_local_content_export_prepare_preserves_bytes() {
    local_exports::export_prepare(&["modrinth", "prism"], false);
}
