use anyhow::{bail, Result};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

use crate::config::types::YamlConfig;

/// A dependency graph where nodes are module names and edges represent dependencies.
pub type ModuleGraph = DiGraph<String, ()>;

/// Build a directed acyclic graph from the module configuration.
///
/// Each node is a module name. An edge from A -> B means B depends on A
/// (A must run before B).
pub fn build_dag(config: &YamlConfig) -> Result<ModuleGraph> {
    let mut graph = DiGraph::new();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    // Add all modules as nodes
    for name in config.project.modules.keys() {
        let idx = graph.add_node(name.clone());
        node_map.insert(name.clone(), idx);
    }

    // Add edges for dependencies
    for (name, module) in &config.project.modules {
        let to_idx = node_map[name];
        for dep in &module.depends_on {
            let from_idx = match node_map.get(dep) {
                Some(idx) => *idx,
                None => bail!("Module '{}' depends on unknown module '{}'", name, dep),
            };
            // Edge from dependency -> dependent (dependency must run first)
            graph.add_edge(from_idx, to_idx, ());
        }
    }

    // Verify it's a DAG (no cycles)
    if petgraph::algo::is_cyclic_directed(&graph) {
        bail!("Circular dependency detected in module graph");
    }

    Ok(graph)
}

/// Get the node map from module name to node index.
pub fn get_node_map(graph: &ModuleGraph) -> HashMap<String, NodeIndex> {
    graph
        .node_indices()
        .map(|idx| (graph[idx].clone(), idx))
        .collect()
}
