use oxid::config::types::YamlModuleConfig;
use oxid::executor::output_parser::{extract_errors, parse_plan_output};
use oxid::executor::terraform::generate_terraform_files;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_generate_terraform_files_basic() {
    let dir = TempDir::new().unwrap();
    let module_dir = dir.path().join("vpc");

    let module_config = YamlModuleConfig {
        source: "terraform-aws-modules/vpc/aws".to_string(),
        version: Some("5.0.0".to_string()),
        depends_on: vec![],
        variables: HashMap::new(),
        outputs: vec!["vpc_id".to_string(), "subnet_ids".to_string()],
    };

    let mut vars = HashMap::new();
    vars.insert(
        "cidr".to_string(),
        serde_json::Value::String("10.0.0.0/16".to_string()),
    );
    vars.insert(
        "name".to_string(),
        serde_json::Value::String("my-vpc".to_string()),
    );

    generate_terraform_files("vpc", &module_config, &vars, &module_dir, Some("us-east-1")).unwrap();

    let main_tf = std::fs::read_to_string(module_dir.join("main.tf")).unwrap();

    assert!(main_tf.contains("terraform-aws-modules/vpc/aws"));
    assert!(main_tf.contains("version = \"5.0.0\""));
    assert!(main_tf.contains("module \"this\""));
    assert!(main_tf.contains("output \"vpc_id\""));
    assert!(main_tf.contains("output \"subnet_ids\""));
    assert!(main_tf.contains("module.this.vpc_id"));
    assert!(main_tf.contains("module.this.subnet_ids"));
    assert!(main_tf.contains("provider \"aws\""));
    assert!(main_tf.contains("region = \"us-east-1\""));
}

#[test]
fn test_generate_terraform_files_no_version() {
    let dir = TempDir::new().unwrap();
    let module_dir = dir.path().join("local-mod");

    let module_config = YamlModuleConfig {
        source: "./modules/my-module".to_string(),
        version: None,
        depends_on: vec![],
        variables: HashMap::new(),
        outputs: vec![],
    };

    let vars = HashMap::new();
    generate_terraform_files("local-mod", &module_config, &vars, &module_dir, None).unwrap();

    let main_tf = std::fs::read_to_string(module_dir.join("main.tf")).unwrap();
    assert!(main_tf.contains("./modules/my-module"));
    assert!(!main_tf.contains("version ="));
}

#[test]
fn test_generate_terraform_with_complex_variables() {
    let dir = TempDir::new().unwrap();
    let module_dir = dir.path().join("complex");

    let module_config = YamlModuleConfig {
        source: "hashicorp/test".to_string(),
        version: Some("1.0.0".to_string()),
        depends_on: vec![],
        variables: HashMap::new(),
        outputs: vec![],
    };

    let mut vars = HashMap::new();
    vars.insert("enabled".to_string(), serde_json::Value::Bool(true));
    vars.insert("count".to_string(), serde_json::Value::Number(42.into()));
    vars.insert("tags".to_string(), serde_json::json!(["a", "b", "c"]));

    generate_terraform_files("complex", &module_config, &vars, &module_dir, None).unwrap();

    let main_tf = std::fs::read_to_string(module_dir.join("main.tf")).unwrap();
    assert!(main_tf.contains("true"));
    assert!(main_tf.contains("42"));
    assert!(main_tf.contains("[\"a\", \"b\", \"c\"]"));
}

#[test]
fn test_parse_plan_output_with_changes() {
    let lines = vec![
        r#"{"@level":"info","@message":"Plan: 3 to add, 1 to change, 0 to destroy.","type":"change_summary"}"#.to_string(),
        r#"{"@level":"info","type":"planned_change","change":{"action":"create","resource":{"addr":"aws_vpc.main"}}}"#.to_string(),
        r#"{"@level":"info","type":"planned_change","change":{"action":"create","resource":{"addr":"aws_subnet.a"}}}"#.to_string(),
        r#"{"@level":"info","type":"planned_change","change":{"action":"update","resource":{"addr":"aws_sg.main"}}}"#.to_string(),
        r#"{"@level":"info","type":"planned_change","change":{"action":"delete","resource":{"addr":"aws_old.remove"}}}"#.to_string(),
    ];

    let summary = parse_plan_output(&lines);
    assert_eq!(summary.to_create, 2);
    assert_eq!(summary.to_update, 1);
    assert_eq!(summary.to_destroy, 1);
}

#[test]
fn test_parse_plan_output_empty() {
    let lines: Vec<String> = vec![];
    let summary = parse_plan_output(&lines);
    assert_eq!(summary.to_create, 0);
    assert_eq!(summary.to_update, 0);
    assert_eq!(summary.to_destroy, 0);
}

#[test]
fn test_extract_errors_from_output() {
    let lines = vec![
        r#"{"@level":"error","diagnostic":{"severity":"error","summary":"Invalid resource type","detail":"No such resource"}}"#.to_string(),
        r#"{"@level":"info","@message":"Apply complete!"}"#.to_string(),
        r#"{"@level":"error","diagnostic":{"severity":"error","summary":"Auth failure"}}"#.to_string(),
    ];

    let errors = extract_errors(&lines);
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0], "Invalid resource type");
    assert_eq!(errors[1], "Auth failure");
}

#[test]
fn test_extract_errors_none() {
    let lines = vec![r#"{"@level":"info","@message":"Apply complete!"}"#.to_string()];

    let errors = extract_errors(&lines);
    assert!(errors.is_empty());
}
