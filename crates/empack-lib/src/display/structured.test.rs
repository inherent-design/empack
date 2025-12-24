use super::*;
use crate::display::test_utils::{create_test_styling, create_test_capabilities};

#[test]
fn test_structured_table_basic_rendering() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Create table with header and rows
    structured
        .table()
        .header(&["Name", "Version", "Status"])
        .row(&["JEI", "15.2.0.27", "✓"])
        .row(&["Fabric API", "0.92.0", "✓"])
        .render();
}

#[test]
fn test_structured_table_without_header() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Table without header
    structured
        .table()
        .row(&["Item 1", "Value 1"])
        .row(&["Item 2", "Value 2"])
        .render();
}

#[test]
fn test_structured_table_empty() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Empty table should not panic
    structured.table().render();
}

#[test]
fn test_structured_pairs_rendering() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Key-value pairs
    let pairs = [
        ("Project", "my-modpack"),
        ("Minecraft", "1.21.6"),
        ("Modloader", "Fabric"),
    ];

    structured.pairs(&pairs);
}

#[test]
fn test_structured_list_rendering() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    let items = vec!["packwiz installed", "tools available", "config loaded"];

    structured.list(&items);
}

#[test]
fn test_structured_numbered_list() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    let items = vec!["First step", "Second step", "Third step"];

    structured.numbered_list(&items);
}

#[test]
fn test_structured_empty_lists() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Empty lists should not panic
    structured.list(&[]);
    structured.numbered_list(&[]);
    structured.pairs(&[]);
}

#[test]
fn test_structured_table_with_max_width() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Table with constrained width
    structured
        .table()
        .header(&["Name", "Description"])
        .row(&["JEI", "Just Enough Items - item and recipe viewing"])
        .row(&["Fabric API", "Core API library for Fabric"])
        .max_width(60)
        .render();
}

#[test]
fn test_structured_table_long_content_truncation() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Very long cell content
    let long_text = "a".repeat(200);

    structured
        .table()
        .header(&["Col1", "Col2"])
        .row(&[&long_text, "short"])
        .max_width(40)
        .render();
}

#[test]
fn test_structured_pairs_alignment() {
    let styling = create_test_styling();
    let caps = create_test_capabilities();
    let structured = StructuredDisplay::new(&styling, &caps);

    // Pairs with varying key lengths should align properly
    let pairs = [
        ("A", "Value 1"),
        ("LongerKey", "Value 2"),
        ("X", "Value 3"),
    ];

    structured.pairs(&pairs);
}
