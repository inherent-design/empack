// Tests for dependency graph resolution

use super::*;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Test Utilities
// ============================================================================

/// Create a test .pw.toml file
fn create_test_toml(dir: &Path, filename: &str, content: &str) -> PathBuf {
    let path = dir.join(filename);
    fs::write(&path, content).unwrap();
    path
}

/// Create a simple mod without dependencies
fn create_simple_mod(dir: &Path, name: &str, mod_id: &str) -> PathBuf {
    let content = format!(
        r#"
name = "{}"
filename = "{}.jar"

[update]
[update.modrinth]
mod-id = "{}"
version = "v1.0.0"
"#,
        name, name, mod_id
    );
    create_test_toml(dir, &format!("{}.pw.toml", name), &content)
}

/// Create a mod with dependencies
fn create_mod_with_deps(
    dir: &Path,
    name: &str,
    mod_id: &str,
    deps: &[(&str, bool)], // (dep_id, is_optional)
) -> PathBuf {
    let mut deps_section = String::from("\n[deps]\n");
    for (dep_id, optional) in deps {
        if *optional {
            deps_section.push_str(&format!(
                "{} = {{ version = \"*\", optional = true }}\n",
                dep_id
            ));
        } else {
            deps_section.push_str(&format!("{} = \"*\"\n", dep_id));
        }
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
// Basic Graph Operations
// ============================================================================

#[test]
fn test_new_graph_is_empty() {
    let graph = DependencyGraph::new();
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_add_single_node() {
    let mut graph = DependencyGraph::new();
    let node = DependencyNode::new(
        "fabric-api".to_string(),
        "Fabric API".to_string(),
        "modrinth".to_string(),
        Some("v1.0.0".to_string()),
    );

    graph.add_node(node.clone());
    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edge_count(), 0);
    assert!(graph.contains("fabric-api"));
}

#[test]
fn test_add_duplicate_node_is_idempotent() {
    let mut graph = DependencyGraph::new();
    let node = DependencyNode::new(
        "fabric-api".to_string(),
        "Fabric API".to_string(),
        "modrinth".to_string(),
        None,
    );

    let idx1 = graph.add_node(node.clone());
    let idx2 = graph.add_node(node);

    assert_eq!(idx1, idx2);
    assert_eq!(graph.node_count(), 1);
}

#[test]
fn test_add_dependency_edge() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new(
        "mod-a".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    );
    let node_b = DependencyNode::new(
        "mod-b".to_string(),
        "Mod B".to_string(),
        "modrinth".to_string(),
        None,
    );

    graph.add_node(node_a);
    graph.add_node(node_b);

    graph
        .add_dependency("mod-a", "mod-b", DependencyType::Required)
        .unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);
}

#[test]
fn test_add_dependency_to_nonexistent_node() {
    let mut graph = DependencyGraph::new();
    let node = DependencyNode::new(
        "mod-a".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    );
    graph.add_node(node);

    let result = graph.add_dependency("mod-a", "nonexistent", DependencyType::Required);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DependencyGraphError::NodeNotFound { .. }
    ));
}

// ============================================================================
// Linear Dependency Chain (A → B → C)
// ============================================================================

#[test]
fn test_linear_dependency_chain() {
    let mut graph = DependencyGraph::new();

    // Create chain: A → B → C
    let node_a = DependencyNode::new(
        "mod-a".to_string(),
        "Mod A".to_string(),
        "modrinth".to_string(),
        None,
    );
    let node_b = DependencyNode::new(
        "mod-b".to_string(),
        "Mod B".to_string(),
        "modrinth".to_string(),
        None,
    );
    let node_c = DependencyNode::new(
        "mod-c".to_string(),
        "Mod C".to_string(),
        "modrinth".to_string(),
        None,
    );

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);

    graph
        .add_dependency("mod-a", "mod-b", DependencyType::Required)
        .unwrap();
    graph
        .add_dependency("mod-b", "mod-c", DependencyType::Required)
        .unwrap();

    assert!(!graph.has_cycles());

    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 3);

    // C should come before B, B before A (reverse dependency order)
    let positions: HashMap<String, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, node)| (node.mod_id.clone(), i))
        .collect();

    assert!(positions["mod-c"] < positions["mod-b"]);
    assert!(positions["mod-b"] < positions["mod-a"]);
}

#[test]
fn test_linear_chain_transitive_dependencies() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("b", "c", DependencyType::Required).unwrap();

    let transitive = graph.get_transitive_dependencies("a").unwrap();
    assert_eq!(transitive.len(), 2); // b and c
}

// ============================================================================
// Diamond Dependency (A → B,C → D)
// ============================================================================

#[test]
fn test_diamond_dependency() {
    let mut graph = DependencyGraph::new();

    // Create diamond: A depends on B and C, both depend on D
    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);
    let node_d = DependencyNode::new("d".to_string(), "D".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);
    graph.add_node(node_d);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("a", "c", DependencyType::Required).unwrap();
    graph.add_dependency("b", "d", DependencyType::Required).unwrap();
    graph.add_dependency("c", "d", DependencyType::Required).unwrap();

    assert!(!graph.has_cycles());

    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 4);

    let positions: HashMap<String, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, node)| (node.mod_id.clone(), i))
        .collect();

    // D must come before both B and C
    assert!(positions["d"] < positions["b"]);
    assert!(positions["d"] < positions["c"]);

    // B and C must come before A
    assert!(positions["b"] < positions["a"]);
    assert!(positions["c"] < positions["a"]);
}

#[test]
fn test_diamond_no_duplicate_in_transitive() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);
    let node_d = DependencyNode::new("d".to_string(), "D".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);
    graph.add_node(node_d);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("a", "c", DependencyType::Required).unwrap();
    graph.add_dependency("b", "d", DependencyType::Required).unwrap();
    graph.add_dependency("c", "d", DependencyType::Required).unwrap();

    let transitive = graph.get_transitive_dependencies("a").unwrap();
    // Should be 3 nodes (b, c, d) - d counted only once
    assert_eq!(transitive.len(), 3);
}

// ============================================================================
// Cycle Detection (A → B → C → A)
// ============================================================================

#[test]
fn test_simple_cycle_detection() {
    let mut graph = DependencyGraph::new();

    // Create cycle: A → B → C → A
    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("b", "c", DependencyType::Required).unwrap();
    graph.add_dependency("c", "a", DependencyType::Required).unwrap();

    assert!(graph.has_cycles());

    let cycle = graph.detect_cycle();
    assert!(cycle.is_some());
    let cycle_path = cycle.unwrap();
    assert!(!cycle_path.is_empty());
}

#[test]
fn test_cycle_prevents_topological_sort() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("b", "c", DependencyType::Required).unwrap();
    graph.add_dependency("c", "a", DependencyType::Required).unwrap();

    let result = graph.topological_sort();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DependencyGraphError::CircularDependency { .. }
    ));
}

#[test]
fn test_self_cycle_detection() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    graph.add_node(node_a);
    graph.add_dependency("a", "a", DependencyType::Required).unwrap();

    assert!(graph.has_cycles());
}

// ============================================================================
// Optional Dependencies
// ============================================================================

#[test]
fn test_optional_dependency() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);

    graph.add_dependency("a", "b", DependencyType::Optional).unwrap();

    let deps = graph.get_dependencies("a").unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].1, DependencyType::Optional);
}

#[test]
fn test_mixed_required_and_optional_dependencies() {
    let mut graph = DependencyGraph::new();

    let node_a = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    let node_b = DependencyNode::new("b".to_string(), "B".to_string(), "m".to_string(), None);
    let node_c = DependencyNode::new("c".to_string(), "C".to_string(), "m".to_string(), None);

    graph.add_node(node_a);
    graph.add_node(node_b);
    graph.add_node(node_c);

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("a", "c", DependencyType::Optional).unwrap();

    let deps = graph.get_dependencies("a").unwrap();
    assert_eq!(deps.len(), 2);

    let required_count = deps
        .iter()
        .filter(|(_, t)| *t == DependencyType::Required)
        .count();
    let optional_count = deps
        .iter()
        .filter(|(_, t)| *t == DependencyType::Optional)
        .count();

    assert_eq!(required_count, 1);
    assert_eq!(optional_count, 1);
}

// ============================================================================
// Packwiz File Parsing
// ============================================================================

#[test]
fn test_parse_simple_packwiz_file() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_simple_mod(temp_dir.path(), "fabric-api", "P7dR8mSH");

    graph
        .parse_packwiz_file(&temp_dir.path().join("fabric-api.pw.toml"))
        .unwrap();

    assert_eq!(graph.node_count(), 1);
    assert!(graph.contains("P7dR8mSH"));

    let node = graph.get_node("P7dR8mSH").unwrap();
    assert_eq!(node.name, "fabric-api");
    assert_eq!(node.platform, "modrinth");
}

#[test]
fn test_parse_packwiz_with_dependencies() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_mod_with_deps(
        temp_dir.path(),
        "mod-menu",
        "mOgUt4GM",
        &[("P7dR8mSH", false)], // Required dependency on fabric-api
    );

    graph
        .parse_packwiz_file(&temp_dir.path().join("mod-menu.pw.toml"))
        .unwrap();

    assert_eq!(graph.node_count(), 2); // mod-menu + fabric-api
    assert_eq!(graph.edge_count(), 1);
    assert!(graph.contains("mOgUt4GM"));
    assert!(graph.contains("P7dR8mSH"));
}

#[test]
fn test_parse_packwiz_with_optional_dependency() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_mod_with_deps(
        temp_dir.path(),
        "jei",
        "jei-mod",
        &[("fabric-api", true)], // Optional
    );

    let node = graph
        .parse_packwiz_file(&temp_dir.path().join("jei.pw.toml"))
        .unwrap();

    // The mod_id comes from the parsed node
    let deps = graph.get_dependencies(&node.mod_id).unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].1, DependencyType::Optional);
}

// ============================================================================
// Build from Directory
// ============================================================================

#[test]
fn test_build_from_directory_single_mod() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_simple_mod(temp_dir.path(), "fabric-api", "P7dR8mSH");

    graph.build_from_directory(temp_dir.path()).unwrap();

    assert_eq!(graph.node_count(), 1);
    assert!(graph.contains("P7dR8mSH"));
}

#[test]
fn test_build_from_directory_multiple_mods() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_simple_mod(temp_dir.path(), "fabric-api", "P7dR8mSH");
    create_simple_mod(temp_dir.path(), "sodium", "AANobbMI");
    create_simple_mod(temp_dir.path(), "lithium", "gvQqBUqZ");

    graph.build_from_directory(temp_dir.path()).unwrap();

    assert_eq!(graph.node_count(), 3);
    assert!(graph.contains("P7dR8mSH"));
    assert!(graph.contains("AANobbMI"));
    assert!(graph.contains("gvQqBUqZ"));
}

#[test]
fn test_build_from_directory_with_dependencies() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_simple_mod(temp_dir.path(), "fabric-api", "P7dR8mSH");
    create_mod_with_deps(
        temp_dir.path(),
        "mod-menu",
        "mOgUt4GM",
        &[("P7dR8mSH", false)],
    );

    graph.build_from_directory(temp_dir.path()).unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);

    // Verify topological sort works
    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 2);

    let positions: HashMap<String, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, node)| (node.mod_id.clone(), i))
        .collect();

    // fabric-api should come before mod-menu
    assert!(positions["P7dR8mSH"] < positions["mOgUt4GM"]);
}

#[test]
fn test_build_from_directory_ignores_non_pw_files() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    create_simple_mod(temp_dir.path(), "fabric-api", "P7dR8mSH");
    // Create a non-.pw.toml file
    create_test_toml(temp_dir.path(), "pack.toml", "name = \"test pack\"");

    graph.build_from_directory(temp_dir.path()).unwrap();

    assert_eq!(graph.node_count(), 1);
    assert!(graph.contains("P7dR8mSH"));
}

// ============================================================================
// Complex Scenarios
// ============================================================================

#[test]
fn test_complex_dependency_tree() {
    let mut graph = DependencyGraph::new();

    // Build a complex tree:
    //     A
    //    / \
    //   B   C
    //  / \ / \
    // D   E   F
    //      \ /
    //       G

    let nodes: Vec<_> = vec!["a", "b", "c", "d", "e", "f", "g"]
        .into_iter()
        .map(|id| {
            DependencyNode::new(id.to_string(), id.to_uppercase(), "m".to_string(), None)
        })
        .collect();

    for node in nodes {
        graph.add_node(node);
    }

    graph.add_dependency("a", "b", DependencyType::Required).unwrap();
    graph.add_dependency("a", "c", DependencyType::Required).unwrap();
    graph.add_dependency("b", "d", DependencyType::Required).unwrap();
    graph.add_dependency("b", "e", DependencyType::Required).unwrap();
    graph.add_dependency("c", "e", DependencyType::Required).unwrap();
    graph.add_dependency("c", "f", DependencyType::Required).unwrap();
    graph.add_dependency("e", "g", DependencyType::Required).unwrap();
    graph.add_dependency("f", "g", DependencyType::Required).unwrap();

    assert!(!graph.has_cycles());

    let sorted = graph.topological_sort().unwrap();
    assert_eq!(sorted.len(), 7);

    let positions: HashMap<String, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, node)| (node.mod_id.clone(), i))
        .collect();

    // Verify ordering constraints
    assert!(positions["g"] < positions["e"]);
    assert!(positions["g"] < positions["f"]);
    assert!(positions["e"] < positions["b"]);
    assert!(positions["e"] < positions["c"]);
    assert!(positions["d"] < positions["b"]);
    assert!(positions["f"] < positions["c"]);
    assert!(positions["b"] < positions["a"]);
    assert!(positions["c"] < positions["a"]);
}

#[test]
fn test_get_node() {
    let mut graph = DependencyGraph::new();
    let node = DependencyNode::new(
        "test-mod".to_string(),
        "Test Mod".to_string(),
        "modrinth".to_string(),
        Some("v1.0.0".to_string()),
    );

    graph.add_node(node.clone());

    let retrieved = graph.get_node("test-mod").unwrap();
    assert_eq!(retrieved.mod_id, "test-mod");
    assert_eq!(retrieved.name, "Test Mod");
    assert_eq!(retrieved.platform, "modrinth");
    assert_eq!(retrieved.version, Some("v1.0.0".to_string()));
}

#[test]
fn test_get_node_nonexistent() {
    let graph = DependencyGraph::new();
    assert!(graph.get_node("nonexistent").is_none());
}

#[test]
fn test_get_dependencies_empty() {
    let mut graph = DependencyGraph::new();
    let node = DependencyNode::new("a".to_string(), "A".to_string(), "m".to_string(), None);
    graph.add_node(node);

    let deps = graph.get_dependencies("a").unwrap();
    assert_eq!(deps.len(), 0);
}

#[test]
fn test_get_dependencies_nonexistent_node() {
    let graph = DependencyGraph::new();
    assert!(graph.get_dependencies("nonexistent").is_none());
}

#[test]
fn test_curseforge_mod_parsing() {
    let temp_dir = TempDir::new().unwrap();
    let mut graph = DependencyGraph::new();

    let content = r#"
name = "JEI"
filename = "jei-1.21.1-forge.jar"

[update]
[update.curseforge]
project-id = 238222
file-id = 5678901
"#;

    create_test_toml(temp_dir.path(), "jei.pw.toml", content);

    graph
        .parse_packwiz_file(&temp_dir.path().join("jei.pw.toml"))
        .unwrap();

    assert_eq!(graph.node_count(), 1);
    assert!(graph.contains("238222"));

    let node = graph.get_node("238222").unwrap();
    assert_eq!(node.name, "JEI");
    assert_eq!(node.platform, "curseforge");
    assert_eq!(node.version, Some("5678901".to_string()));
}
