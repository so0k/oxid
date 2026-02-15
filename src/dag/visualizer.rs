use petgraph::graph::DiGraph;

/// Convert the module dependency graph to DOT format for visualization.
pub fn to_dot(graph: &DiGraph<String, ()>) -> String {
    let mut lines = Vec::new();
    lines.push("digraph oxid {".to_string());
    lines.push("    rankdir=TB;".to_string());
    lines.push("    node [shape=box, style=filled, fillcolor=lightblue];".to_string());

    for idx in graph.node_indices() {
        lines.push(format!(
            "    \"{}\" [label=\"{}\"];",
            graph[idx], graph[idx]
        ));
    }

    for edge in graph.edge_indices() {
        let (from, to) = graph.edge_endpoints(edge).unwrap();
        lines.push(format!("    \"{}\" -> \"{}\";", graph[from], graph[to]));
    }

    lines.push("}".to_string());
    lines.join("\n")
}
