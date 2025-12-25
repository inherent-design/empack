// Tests for dependency cycle detection

use super::*;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Test Utilities
// ============================================================================

/// Create a test .pw.toml file
fn create_test_toml(dir: &std::path::Path, filename: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(filename);
    fs::write(&path, content).unwrap();
    path
}

/// Create a mod with dependencies (circular)
fn create_circular_mod(
    dir: &std::path::Path,
    name: &str,
    mod_id: &str,
    deps: &[&str],
) -> std::path::PathBuf {
    let mut deps_section = String::from("\n[deps]\n");
    for dep_id in deps {
        deps_section.push_str(&format!("{} = \"*\"\n", dep_id));
    }

    let content = format!(
        r#"
name = "{}"
filename = "{}.jar"

[update]
[update.modrinth]
mod-id = "{}"
version = "v1.0.0"
{}
"#,
        name, name, mod_id, deps_section
    );
    create_test_toml(dir, &format!("{}.pw.toml", name), &content)
}

// ============================================================================
// Cycle Detection Tests
// ============================================================================

#[test]
fn test_detect_simple_cycle_two_nodes() {
    // Create a circular dependency: A -> B -> A
    let temp_dir = TempDir::new().unwrap();

    create_circular_mod(temp_dir.path(), "mod-a", "mod-a-id", &["mod-b-id"]);
    create_circular_mod(temp_dir.path(), "mod-b", "mod-b-id", &["mod-a-id"]);

    let mut graph = DependencyGraph::new();

    // Add nodes
    graph.add_node(DependencyNode::new(
        "mod-a-id".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-b-id".to_string(),
        "Mod B".to_string(),
        "modrinth".to_string(),
        None,
    ));

    // Add circular edges
    graph.add_dependency("mod-a-id", "mod-b-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-b-id", "mod-a-id", DependencyType::Required).unwrap();

    // Verify cycle is detected
    assert!(graph.has_cycles(), "Should detect cycle in A -> B -> A");

    // Verify cycle path is returned
    let cycle_path = graph.detect_cycle();
    assert!(cycle_path.is_some(), "detect_cycle should return Some for cyclic graph");

    let cycle = cycle_path.unwrap();
    // Error message should mention both mods
    let cycle_str = cycle.join(" -> ");
    assert!(
        cycle_str.contains("mod-a-id") || cycle_str.contains("Mod A"),
        "Cycle path should mention mod-a: {}",
        cycle_str
    );
    assert!(
        cycle_str.contains("mod-b-id") || cycle_str.contains("Mod B"),
        "Cycle path should mention mod-b: {}",
        cycle_str
    );
}

#[test]
fn test_detect_longer_cycle_three_nodes() {
    // Create a circular dependency: A -> B -> C -> A
    let temp_dir = TempDir::new().unwrap();

    create_circular_mod(temp_dir.path(), "mod-a", "mod-a-id", &["mod-b-id"]);
    create_circular_mod(temp_dir.path(), "mod-b", "mod-b-id", &["mod-c-id"]);
    create_circular_mod(temp_dir.path(), "mod-c", "mod-c-id", &["mod-a-id"]);

    let mut graph = DependencyGraph::new();

    // Add nodes
    graph.add_node(DependencyNode::new(
        "mod-a-id".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-b-id".to_string(),
        "Mod B".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-c-id".to_string(),
        "Mod C".to_string(),
        "modrinth".to_string(),
        None,
    ));

    // Add circular edges
    graph.add_dependency("mod-a-id", "mod-b-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-b-id", "mod-c-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-c-id", "mod-a-id", DependencyType::Required).unwrap();

    // Verify cycle is detected
    assert!(graph.has_cycles(), "Should detect cycle in A -> B -> C -> A");

    // Verify cycle path is returned
    let cycle_path = graph.detect_cycle();
    assert!(cycle_path.is_some(), "detect_cycle should return Some for cyclic graph");

    let cycle = cycle_path.unwrap();
    // Error message should be informative (contain at least one mod)
    assert!(!cycle.is_empty(), "Cycle path should not be empty");

    // Should mention at least one of the mods in the cycle
    let cycle_str = cycle.join(" -> ");
    let mentions_mod = cycle_str.contains("mod-a-id")
        || cycle_str.contains("mod-b-id")
        || cycle_str.contains("mod-c-id")
        || cycle_str.contains("Mod A")
        || cycle_str.contains("Mod B")
        || cycle_str.contains("Mod C");
    assert!(
        mentions_mod,
        "Cycle path should mention at least one mod: {}",
        cycle_str
    );
}

#[test]
fn test_no_cycle_in_dag() {
    // Create a DAG (Directed Acyclic Graph): A -> B, A -> C, B -> D, C -> D
    let mut graph = DependencyGraph::new();

    // Add nodes
    graph.add_node(DependencyNode::new(
        "mod-a-id".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-b-id".to_string(),
        "Mod B".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-c-id".to_string(),
        "Mod C".to_string(),
        "modrinth".to_string(),
        None,
    ));
    graph.add_node(DependencyNode::new(
        "mod-d-id".to_string(),
        "Mod D".to_string(),
        "modrinth".to_string(),
        None,
    ));

    // Add edges (DAG structure)
    graph.add_dependency("mod-a-id", "mod-b-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-a-id", "mod-c-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-b-id", "mod-d-id", DependencyType::Required).unwrap();
    graph.add_dependency("mod-c-id", "mod-d-id", DependencyType::Required).unwrap();

    // Verify no cycle
    assert!(!graph.has_cycles(), "DAG should not have cycles");

    // Verify detect_cycle returns None for DAG
    let cycle_path = graph.detect_cycle();
    assert!(cycle_path.is_none(), "detect_cycle should return None for DAG");
}

#[test]
fn test_self_cycle_detection() {
    // Create a self-referencing dependency: A -> A
    let mut graph = DependencyGraph::new();

    graph.add_node(DependencyNode::new(
        "mod-a-id".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    ));

    // Add self-referencing edge
    graph.add_dependency("mod-a-id", "mod-a-id", DependencyType::Required).unwrap();

    // Verify self-cycle is detected
    assert!(
        graph.has_cycles(),
        "Should detect self-referencing cycle A -> A"
    );

    // Verify cycle path is returned
    let cycle_path = graph.detect_cycle();
    assert!(cycle_path.is_some(), "detect_cycle should return Some for self-cycle");

    let cycle = cycle_path.unwrap();
    let cycle_str = cycle.join(" -> ");
    assert!(
        cycle_str.contains("mod-a-id") || cycle_str.contains("Mod A"),
        "Cycle error should mention mod-a: {}",
        cycle_str
    );
}
