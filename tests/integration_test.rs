use oxid::config::parser::parse_config;
use oxid::config::validator::validate;
use oxid::dag::builder::build_dag;
use oxid::dag::resolver::resolve_batches;
use oxid::executor::terraform::generate_terraform_files;
use oxid::planner::diff::detect_drift;
use oxid::planner::plan::ExecutionPlan;
use oxid::state::store::StateStore;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_full_pipeline_config_to_plan() {
    let yaml = r#"
project:
  name: "integration-test"
  version: "1.0"
  variables:
    region: "us-east-1"
    env: "test"
  modules:
    vpc:
      source: "terraform-aws-modules/vpc/aws"
      version: "5.0.0"
      variables:
        name: "${var.env}-vpc"
        cidr: "10.0.0.0/16"
      outputs:
        - vpc_id
    sg:
      source: "./modules/sg"
      depends_on:
        - vpc
      variables:
        vpc_id: "${module.vpc.vpc_id}"
        env: "${var.env}"
      outputs:
        - sg_id
    db:
      source: "terraform-aws-modules/rds/aws"
      version: "6.0.0"
      depends_on:
        - vpc
        - sg
      variables:
        subnet_ids: "${module.vpc.subnet_ids}"
    eks:
      source: "terraform-aws-modules/eks/aws"
      version: "19.0.0"
      depends_on:
        - vpc
        - sg
      variables:
        vpc_id: "${module.vpc.vpc_id}"
    app:
      source: "./modules/app"
      depends_on:
        - eks
        - db
      variables:
        env: "${var.env}"
"#;

    // Parse
    let config = parse_config(yaml).unwrap();

    // Validate
    validate(&config).unwrap();

    // Build DAG
    let graph = build_dag(&config).unwrap();

    // Resolve batches
    let batches = resolve_batches(&graph);
    assert_eq!(batches.len(), 4);

    // Batch 1: vpc
    assert_eq!(batches[0], vec!["vpc"]);
    // Batch 2: sg
    assert_eq!(batches[1], vec!["sg"]);
    // Batch 3: db + eks (parallel)
    assert_eq!(batches[2].len(), 2);
    assert!(batches[2].contains(&"db".to_string()));
    assert!(batches[2].contains(&"eks".to_string()));
    // Batch 4: app
    assert_eq!(batches[3], vec!["app"]);

    // Build plan
    let plan = ExecutionPlan::from_batches(&config, &batches);
    assert_eq!(plan.total_modules, 5);
    assert_eq!(plan.batches.len(), 4);
}

#[test]
fn test_terraform_file_generation_and_state() {
    let dir = TempDir::new().unwrap();
    let working_dir = dir.path().to_str().unwrap();

    // Initialize state
    let store = StateStore::open(working_dir).unwrap();
    store.initialize().unwrap();

    // Generate terraform files
    let module_dir = dir.path().join("modules").join("vpc");
    let module_config = oxid::config::types::YamlModuleConfig {
        source: "terraform-aws-modules/vpc/aws".to_string(),
        version: Some("5.0.0".to_string()),
        depends_on: vec![],
        variables: HashMap::new(),
        outputs: vec!["vpc_id".to_string()],
    };

    let mut vars = HashMap::new();
    vars.insert(
        "name".to_string(),
        serde_json::Value::String("test-vpc".to_string()),
    );
    vars.insert(
        "cidr".to_string(),
        serde_json::Value::String("10.0.0.0/16".to_string()),
    );

    generate_terraform_files("vpc", &module_config, &vars, &module_dir, Some("us-east-1")).unwrap();

    // Verify file exists
    assert!(module_dir.join("main.tf").exists());

    // Verify content
    let content = std::fs::read_to_string(module_dir.join("main.tf")).unwrap();
    assert!(content.contains("terraform-aws-modules/vpc/aws"));
    assert!(content.contains("version = \"5.0.0\""));
    assert!(content.contains("output \"vpc_id\""));

    // Track state
    store.update_module_status("vpc", "succeeded").unwrap();
    store.set_output("vpc", "vpc_id", "vpc-test-123").unwrap();

    // Verify state
    let status = store.get_module_status("vpc").unwrap().unwrap();
    assert_eq!(status, "succeeded");

    let output = store.get_output("vpc", "vpc_id").unwrap().unwrap();
    assert_eq!(output, "vpc-test-123");
}

#[test]
fn test_drift_detection_new_modules() {
    let dir = TempDir::new().unwrap();
    let working_dir = dir.path().to_str().unwrap();

    let store = StateStore::open(working_dir).unwrap();
    store.initialize().unwrap();

    let yaml = r#"
project:
  name: "drift-test"
  version: "1.0"
  modules:
    vpc:
      source: "./vpc"
    sg:
      source: "./sg"
      depends_on:
        - vpc
"#;

    let config = parse_config(yaml).unwrap();
    let drifts = detect_drift(&config, &store).unwrap();

    // Both modules should be detected as new (not in state)
    assert_eq!(drifts.len(), 2);
}

#[test]
fn test_drift_detection_removed_module() {
    let dir = TempDir::new().unwrap();
    let working_dir = dir.path().to_str().unwrap();

    let store = StateStore::open(working_dir).unwrap();
    store.initialize().unwrap();

    // Module exists in state but not in config
    store
        .update_module_status("old_module", "succeeded")
        .unwrap();

    let yaml = r#"
project:
  name: "drift-test"
  version: "1.0"
  modules:
    vpc:
      source: "./vpc"
"#;

    let config = parse_config(yaml).unwrap();
    let drifts = detect_drift(&config, &store).unwrap();

    // Should detect: vpc is new, old_module is removed
    assert_eq!(drifts.len(), 2);

    let removed = drifts
        .iter()
        .find(|d| d.module_name == "old_module")
        .unwrap();
    assert!(matches!(
        removed.drift_type,
        oxid::planner::diff::DriftType::RemovedModule
    ));
}

#[test]
fn test_drift_detection_inconsistent_state() {
    let dir = TempDir::new().unwrap();
    let working_dir = dir.path().to_str().unwrap();

    let store = StateStore::open(working_dir).unwrap();
    store.initialize().unwrap();

    store.update_module_status("vpc", "failed").unwrap();

    let yaml = r#"
project:
  name: "drift-test"
  version: "1.0"
  modules:
    vpc:
      source: "./vpc"
"#;

    let config = parse_config(yaml).unwrap();
    let drifts = detect_drift(&config, &store).unwrap();

    // vpc is in state but with failed status
    let inconsistent = drifts.iter().find(|d| d.module_name == "vpc").unwrap();
    assert!(matches!(
        inconsistent.drift_type,
        oxid::planner::diff::DriftType::StateInconsistent
    ));
}

#[test]
fn test_validate_command_with_example_config() {
    let yaml = std::fs::read_to_string("templates/example-project.yaml").unwrap();
    let config = parse_config(&yaml).unwrap();
    validate(&config).unwrap();

    let graph = build_dag(&config).unwrap();
    let batches = resolve_batches(&graph);

    // Should match spec: 4 batches
    assert_eq!(batches.len(), 4);
    assert_eq!(batches[0], vec!["vpc"]);
    assert_eq!(batches[1], vec!["security_groups"]);
    assert_eq!(batches[2].len(), 2); // database + eks_cluster
    assert_eq!(batches[3], vec!["app_deployment"]);
}
