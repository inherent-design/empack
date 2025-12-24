//! Dependency graph resolution with transitive dependencies and cycle detection
//!
//! This module provides a graph-based dependency resolver for project management.
//! It parses packwiz `.pw.toml` files, builds a dependency graph, detects cycles,
//! and provides topological ordering for installation.

use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use toml::Value;
use tracing::{debug, trace};

/// Errors that can occur during dependency resolution
#[derive(Debug, Error)]
pub enum DependencyGraphError {
    #[error("Circular dependency detected: {cycle}")]
    CircularDependency { cycle: String },

    #[error("Missing dependency: {mod_name} required by {required_by}")]
    MissingDependency {
        mod_name: String,
        required_by: String,
    },

    #[error("Failed to read file: {path}: {source}")]
    FileReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse TOML: {path}: {source}")]
    TomlParseError {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Invalid dependency format in {path}")]
    InvalidDependencyFormat { path: PathBuf },

    #[error("Node not found: {mod_id}")]
    NodeNotFound { mod_id: String },
}

/// Dependency relationship types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyType {
    /// Required dependency (installation fails without it)
    Required,
    /// Optional dependency (installation proceeds without it)
    Optional,
}

/// Represents a project in the dependency graph
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyNode {
    /// Unique mod identifier (slug or ID)
    pub mod_id: String,
    /// Display name of the mod
    pub name: String,
    /// Platform (modrinth, curseforge, forge)
    pub platform: String,
    /// Specific version (if pinned)
    pub version: Option<String>,
    /// Path to the .pw.toml file (if local)
    pub source_path: Option<PathBuf>,
}

impl DependencyNode {
    /// Create a new dependency node
    pub fn new(
        mod_id: String,
        name: String,
        platform: String,
        version: Option<String>,
    ) -> Self {
        Self {
            mod_id,
            name,
            platform,
            version,
            source_path: None,
        }
    }

    /// Create node with source path
    pub fn with_source(mut self, path: PathBuf) -> Self {
        self.source_path = Some(path);
        self
    }
}

/// Dependency graph for transitive dependency resolution
pub struct DependencyGraph {
    /// Directed graph: nodes = mods, edges = dependencies
    graph: DiGraph<DependencyNode, DependencyType>,
    /// Map from mod_id to node index for fast lookup
    node_map: HashMap<String, NodeIndex>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_map: HashMap::new(),
        }
    }

    /// Add a project to the graph (idempotent - won't duplicate if already exists)
    pub fn add_node(&mut self, node: DependencyNode) -> NodeIndex {
        if let Some(&idx) = self.node_map.get(&node.mod_id) {
            trace!("Node already exists: {}", node.mod_id);
            return idx;
        }

        let mod_id = node.mod_id.clone();
        let idx = self.graph.add_node(node);
        self.node_map.insert(mod_id, idx);
        idx
    }

    /// Add a dependency edge between two mods
    /// Note: Edge direction is from dependency TO dependent (to_id must be installed before from_id)
    pub fn add_dependency(
        &mut self,
        from_id: &str,
        to_id: &str,
        dep_type: DependencyType,
    ) -> Result<(), DependencyGraphError> {
        let from_idx = self
            .node_map
            .get(from_id)
            .ok_or_else(|| DependencyGraphError::NodeNotFound {
                mod_id: from_id.to_string(),
            })?;

        let to_idx = self
            .node_map
            .get(to_id)
            .ok_or_else(|| DependencyGraphError::NodeNotFound {
                mod_id: to_id.to_string(),
            })?;

        // Edge from dependency (to) to dependent (from) - this ensures topological sort
        // returns dependencies before their dependents
        self.graph.add_edge(*to_idx, *from_idx, dep_type);
        Ok(())
    }

    /// Check if the graph contains cycles
    pub fn has_cycles(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// Detect and return cycle path if exists
    pub fn detect_cycle(&self) -> Option<Vec<String>> {
        if !self.has_cycles() {
            return None;
        }

        // Use DFS to find a cycle
        let mut visited = HashMap::new();
        let mut stack = Vec::new();

        for node_idx in self.graph.node_indices() {
            if !visited.contains_key(&node_idx) {
                if let Some(cycle) = self.dfs_cycle_detect(node_idx, &mut visited, &mut stack) {
                    return Some(cycle);
                }
            }
        }

        None
    }

    /// DFS-based cycle detection
    fn dfs_cycle_detect(
        &self,
        node: NodeIndex,
        visited: &mut HashMap<NodeIndex, bool>,
        stack: &mut Vec<NodeIndex>,
    ) -> Option<Vec<String>> {
        visited.insert(node, true);
        stack.push(node);

        for neighbor in self.graph.neighbors(node) {
            if !visited.contains_key(&neighbor) {
                if let Some(cycle) = self.dfs_cycle_detect(neighbor, visited, stack) {
                    return Some(cycle);
                }
            } else if stack.contains(&neighbor) {
                // Found cycle - extract path
                let cycle_start_pos = stack.iter().position(|&n| n == neighbor).unwrap();
                let cycle_nodes: Vec<String> = stack[cycle_start_pos..]
                    .iter()
                    .map(|&idx| self.graph[idx].mod_id.clone())
                    .collect();
                return Some(cycle_nodes);
            }
        }

        stack.pop();
        Some(Vec::new()) // No cycle found in this path
    }

    /// Get topological sort order (installation order)
    /// Returns error if graph has cycles
    pub fn topological_sort(&self) -> Result<Vec<DependencyNode>, DependencyGraphError> {
        if let Some(cycle) = self.detect_cycle() {
            return Err(DependencyGraphError::CircularDependency {
                cycle: cycle.join(" → "),
            });
        }

        let sorted_indices = toposort(&self.graph, None).map_err(|_| {
            DependencyGraphError::CircularDependency {
                cycle: "unknown".to_string(),
            }
        })?;

        Ok(sorted_indices
            .into_iter()
            .map(|idx| self.graph[idx].clone())
            .collect())
    }

    /// Get all direct dependencies of a mod
    pub fn get_dependencies(&self, mod_id: &str) -> Option<Vec<(DependencyNode, DependencyType)>> {
        let node_idx = self.node_map.get(mod_id)?;

        // Since edges go from dependency -> dependent, we need to look at incoming edges
        // to find what this mod depends on
        let deps = self
            .graph
            .edges_directed(*node_idx, petgraph::Direction::Incoming)
            .map(|edge| {
                let source_node = &self.graph[edge.source()];
                (source_node.clone(), *edge.weight())
            })
            .collect();

        Some(deps)
    }

    /// Get all transitive dependencies (recursively)
    pub fn get_transitive_dependencies(
        &self,
        mod_id: &str,
    ) -> Option<Vec<DependencyNode>> {
        let node_idx = self.node_map.get(mod_id)?;
        let mut visited = HashMap::new();
        let mut result = Vec::new();

        self.collect_transitive(*node_idx, &mut visited, &mut result);
        Some(result)
    }

    /// Recursive helper for transitive dependency collection
    fn collect_transitive(
        &self,
        node: NodeIndex,
        visited: &mut HashMap<NodeIndex, bool>,
        result: &mut Vec<DependencyNode>,
    ) {
        // Follow incoming edges (dependencies)
        for neighbor in self.graph.neighbors_directed(node, petgraph::Direction::Incoming) {
            if !visited.contains_key(&neighbor) {
                visited.insert(neighbor, true);
                result.push(self.graph[neighbor].clone());
                self.collect_transitive(neighbor, visited, result);
            }
        }
    }

    /// Get all direct dependents of a mod (mods that depend on this one)
    /// This is the reverse of `get_dependencies` - useful for orphan detection
    pub fn get_dependents(&self, mod_id: &str) -> Option<Vec<DependencyNode>> {
        let node_idx = self.node_map.get(mod_id)?;

        // Since edges go from dependency -> dependent, we need to look at outgoing edges
        // to find what mods depend on this one
        let dependents = self
            .graph
            .edges_directed(*node_idx, petgraph::Direction::Outgoing)
            .map(|edge| {
                let target_node = &self.graph[edge.target()];
                target_node.clone()
            })
            .collect();

        Some(dependents)
    }

    /// Parse a packwiz `.pw.toml` file and extract dependencies
    pub fn parse_packwiz_file(
        &mut self,
        path: &Path,
    ) -> Result<DependencyNode, DependencyGraphError> {
        trace!("Parsing packwiz file: {}", path.display());

        let content = std::fs::read_to_string(path).map_err(|e| {
            DependencyGraphError::FileReadError {
                path: path.to_path_buf(),
                source: e,
            }
        })?;

        let toml: Value = toml::from_str(&content).map_err(|e| {
            DependencyGraphError::TomlParseError {
                path: path.to_path_buf(),
                source: e,
            }
        })?;

        // Extract project metadata
        let name = toml
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Determine platform and mod_id from update section
        let (platform, mod_id, version) = if let Some(update) = toml.get("update") {
            if let Some(modrinth) = update.get("modrinth") {
                let mod_id = modrinth
                    .get("mod-id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let version = modrinth
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                ("modrinth".to_string(), mod_id, version)
            } else if let Some(curseforge) = update.get("curseforge") {
                let mod_id = curseforge
                    .get("project-id")
                    .and_then(|v| v.as_integer())
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                let version = curseforge
                    .get("file-id")
                    .and_then(|v| v.as_integer())
                    .map(|i| i.to_string());
                ("curseforge".to_string(), mod_id, version)
            } else {
                ("unknown".to_string(), name.clone(), None)
            }
        } else {
            ("unknown".to_string(), name.clone(), None)
        };

        let node = DependencyNode::new(mod_id.clone(), name, platform, version)
            .with_source(path.to_path_buf());

        let node_idx = self.add_node(node.clone());

        // Parse dependencies section
        if let Some(deps) = toml.get("deps").and_then(|t| t.as_table()) {
            for (dep_id, dep_value) in deps {
                trace!("Found dependency: {} = {:?}", dep_id, dep_value);

                // Dependency can be:
                // - String: version constraint (required)
                // - Table: { version = "x.y.z", optional = true }
                let (dep_type, _version_constraint) = match dep_value {
                    Value::String(v) => (DependencyType::Required, Some(v.as_str())),
                    Value::Table(t) => {
                        let optional = t
                            .get("optional")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let version = t.get("version").and_then(|v| v.as_str());
                        let typ = if optional {
                            DependencyType::Optional
                        } else {
                            DependencyType::Required
                        };
                        (typ, version)
                    }
                    _ => continue,
                };

                // Create dependency node (minimal - will be filled in later)
                let dep_node = DependencyNode::new(
                    dep_id.clone(),
                    dep_id.clone(),
                    "unknown".to_string(),
                    None,
                );
                let dep_idx = self.add_node(dep_node);

                // Add edge from dependency to dependent (reversed for topological sort)
                self.graph.add_edge(dep_idx, node_idx, dep_type);
            }
        }

        Ok(node)
    }

    /// Build graph from a directory of packwiz files
    pub fn build_from_directory(&mut self, dir: &Path) -> Result<(), DependencyGraphError> {
        debug!("Building dependency graph from: {}", dir.display());

        let entries = std::fs::read_dir(dir).map_err(|e| {
            DependencyGraphError::FileReadError {
                path: dir.to_path_buf(),
                source: e,
            }
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| DependencyGraphError::FileReadError {
                path: dir.to_path_buf(),
                source: e,
            })?;

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml")
                && path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.ends_with(".pw.toml"))
                    .unwrap_or(false)
            {
                self.parse_packwiz_file(&path)?;
            }
        }

        Ok(())
    }

    /// Get the number of nodes in the graph
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Get the number of edges in the graph
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Check if a mod exists in the graph
    pub fn contains(&self, mod_id: &str) -> bool {
        self.node_map.contains_key(mod_id)
    }

    /// Get a node by mod_id
    pub fn get_node(&self, mod_id: &str) -> Option<&DependencyNode> {
        let idx = self.node_map.get(mod_id)?;
        Some(&self.graph[*idx])
    }

    /// Get an iterator over all nodes in the graph
    pub fn all_nodes(&self) -> impl Iterator<Item = &DependencyNode> {
        self.graph.node_weights()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    include!("dependency_graph.test.rs");
}
