use petgraph::graph::DiGraph;
use std::collections::{HashMap, VecDeque};

/// Resolve the dependency graph into parallel execution batches.
///
/// Each batch contains modules that can run concurrently (all their
/// dependencies are in previous batches). Uses Kahn's algorithm.
pub fn resolve_batches(graph: &DiGraph<String, ()>) -> Vec<Vec<String>> {
    let mut in_degree: HashMap<petgraph::graph::NodeIndex, usize> = HashMap::new();
    let mut adjacency: HashMap<petgraph::graph::NodeIndex, Vec<petgraph::graph::NodeIndex>> =
        HashMap::new();

    for idx in graph.node_indices() {
        in_degree.insert(idx, 0);
        adjacency.insert(idx, Vec::new());
    }

    for edge in graph.edge_indices() {
        let (from, to) = graph.edge_endpoints(edge).unwrap();
        adjacency.entry(from).or_default().push(to);
        *in_degree.entry(to).or_insert(0) += 1;
    }

    let mut batches: Vec<Vec<String>> = Vec::new();
    let mut queue: VecDeque<petgraph::graph::NodeIndex> = VecDeque::new();

    // Start with nodes that have no dependencies
    for (&idx, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(idx);
        }
    }

    while !queue.is_empty() {
        let mut batch = Vec::new();
        let mut next_queue = VecDeque::new();

        // All nodes currently in the queue form one parallel batch
        while let Some(node) = queue.pop_front() {
            batch.push(graph[node].clone());

            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    let deg = in_degree.get_mut(&neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push_back(neighbor);
                    }
                }
            }
        }

        batch.sort(); // Deterministic ordering within a batch
        batches.push(batch);
        queue = next_queue;
    }

    batches
}
