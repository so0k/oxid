use oxid::config::parser::parse_config;
use oxid::dag::builder::build_dag;
use oxid::dag::resolver::resolve_batches;
use oxid::dag::visualizer::to_dot;

#[test]
fn test_single_module_dag() {
    let yaml = r#"
project:
  name: "single"
  version: "1.0"
  modules:
    vpc:
      source: "./vpc"
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0], vec!["vpc"]);
}

#[test]
fn test_linear_dependency_chain() {
    let yaml = r#"
project:
  name: "linear"
  version: "1.0"
  modules:
    a:
      source: "./a"
    b:
      source: "./b"
      depends_on:
        - a
    c:
      source: "./c"
      depends_on:
        - b
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0], vec!["a"]);
    assert_eq!(batches[1], vec!["b"]);
    assert_eq!(batches[2], vec!["c"]);
}

#[test]
fn test_parallel_independent_modules() {
    let yaml = r#"
project:
  name: "parallel"
  version: "1.0"
  modules:
    a:
      source: "./a"
    b:
      source: "./b"
    c:
      source: "./c"
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 3);
    // Should be sorted alphabetically within batch
    assert_eq!(batches[0], vec!["a", "b", "c"]);
}

#[test]
fn test_diamond_dependency() {
    // A -> B, A -> C, B -> D, C -> D
    let yaml = r#"
project:
  name: "diamond"
  version: "1.0"
  modules:
    a:
      source: "./a"
    b:
      source: "./b"
      depends_on:
        - a
    c:
      source: "./c"
      depends_on:
        - a
    d:
      source: "./d"
      depends_on:
        - b
        - c
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0], vec!["a"]);
    assert_eq!(batches[1], vec!["b", "c"]); // parallel
    assert_eq!(batches[2], vec!["d"]);
}

#[test]
fn test_complex_dag_from_spec() {
    // Matches the example in the spec:
    // vpc -> sg -> {db, eks} -> app
    let yaml = r#"
project:
  name: "spec-example"
  version: "1.0"
  modules:
    vpc:
      source: "terraform-aws-modules/vpc/aws"
    security_groups:
      source: "./modules/sg"
      depends_on:
        - vpc
    database:
      source: "terraform-aws-modules/rds/aws"
      depends_on:
        - vpc
        - security_groups
    eks_cluster:
      source: "terraform-aws-modules/eks/aws"
      depends_on:
        - vpc
        - security_groups
    app_deployment:
      source: "./modules/app"
      depends_on:
        - eks_cluster
        - database
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 4, "Should have 4 batches: {:?}", batches);

    // Batch 1: vpc (no deps)
    assert_eq!(batches[0], vec!["vpc"]);

    // Batch 2: security_groups
    assert_eq!(batches[1], vec!["security_groups"]);

    // Batch 3: database + eks_cluster (parallel)
    assert_eq!(batches[2].len(), 2);
    assert!(batches[2].contains(&"database".to_string()));
    assert!(batches[2].contains(&"eks_cluster".to_string()));

    // Batch 4: app_deployment
    assert_eq!(batches[3], vec!["app_deployment"]);
}

#[test]
fn test_cycle_detection_in_dag() {
    let yaml = r#"
project:
  name: "cycle"
  version: "1.0"
  modules:
    a:
      source: "./a"
      depends_on:
        - b
    b:
      source: "./b"
      depends_on:
        - a
"#;

    let config = parse_config(yaml).unwrap();
    let result = build_dag(&config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Circular"));
}

#[test]
fn test_unknown_dependency_in_dag() {
    let yaml = r#"
project:
  name: "bad-dep"
  version: "1.0"
  modules:
    a:
      source: "./a"
      depends_on:
        - nonexistent
"#;

    let config = parse_config(yaml).unwrap();
    let result = build_dag(&config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("nonexistent"));
}

#[test]
fn test_dot_output() {
    let yaml = r#"
project:
  name: "dot-test"
  version: "1.0"
  modules:
    a:
      source: "./a"
    b:
      source: "./b"
      depends_on:
        - a
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let dot = to_dot(&graph);

    assert!(dot.contains("digraph oxid"));
    assert!(dot.contains("\"a\""));
    assert!(dot.contains("\"b\""));
    assert!(dot.contains("->"));
}

#[test]
fn test_wide_parallel_dag() {
    // One root with many independent children
    let yaml = r#"
project:
  name: "wide"
  version: "1.0"
  modules:
    root:
      source: "./root"
    child1:
      source: "./c1"
      depends_on: [root]
    child2:
      source: "./c2"
      depends_on: [root]
    child3:
      source: "./c3"
      depends_on: [root]
    child4:
      source: "./c4"
      depends_on: [root]
    child5:
      source: "./c5"
      depends_on: [root]
"#;

    let config = parse_config(yaml).unwrap();
    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0], vec!["root"]);
    assert_eq!(batches[1].len(), 5); // All children in parallel
}
