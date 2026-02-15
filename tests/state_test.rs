use oxid::state::store::StateStore;
use tempfile::TempDir;

fn create_test_store() -> (TempDir, StateStore) {
    let dir = TempDir::new().unwrap();
    let store = StateStore::open(dir.path().to_str().unwrap()).unwrap();
    store.initialize().unwrap();
    (dir, store)
}

#[test]
fn test_initialize_creates_tables() {
    let (_dir, store) = create_test_store();
    // Should not error on second init
    store.initialize().unwrap();
}

#[test]
fn test_update_and_get_module_status() {
    let (_dir, store) = create_test_store();

    // Initially no status
    let status = store.get_module_status("vpc").unwrap();
    assert!(status.is_none());

    // Set status
    store.update_module_status("vpc", "running").unwrap();
    let status = store.get_module_status("vpc").unwrap();
    assert_eq!(status.unwrap(), "running");

    // Update status
    store.update_module_status("vpc", "succeeded").unwrap();
    let status = store.get_module_status("vpc").unwrap();
    assert_eq!(status.unwrap(), "succeeded");
}

#[test]
fn test_list_modules() {
    let (_dir, store) = create_test_store();

    store.update_module_status("vpc", "succeeded").unwrap();
    store.update_module_status("sg", "failed").unwrap();
    store.update_module_status("db", "pending").unwrap();

    let modules = store.list_modules().unwrap();
    assert_eq!(modules.len(), 3);

    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"vpc"));
    assert!(names.contains(&"sg"));
    assert!(names.contains(&"db"));
}

#[test]
fn test_set_and_get_output() {
    let (_dir, store) = create_test_store();

    store.set_output("vpc", "vpc_id", "vpc-12345").unwrap();
    store
        .set_output("vpc", "subnet_ids", "[\"subnet-a\",\"subnet-b\"]")
        .unwrap();

    let vpc_id = store.get_output("vpc", "vpc_id").unwrap();
    assert_eq!(vpc_id.unwrap(), "vpc-12345");

    let subnets = store.get_output("vpc", "subnet_ids").unwrap();
    assert_eq!(subnets.unwrap(), "[\"subnet-a\",\"subnet-b\"]");

    // Nonexistent output
    let missing = store.get_output("vpc", "nonexistent").unwrap();
    assert!(missing.is_none());

    // Nonexistent module
    let missing = store.get_output("nonexistent", "vpc_id").unwrap();
    assert!(missing.is_none());
}

#[test]
fn test_update_output_overwrites() {
    let (_dir, store) = create_test_store();

    store.set_output("vpc", "vpc_id", "old-id").unwrap();
    store.set_output("vpc", "vpc_id", "new-id").unwrap();

    let result = store.get_output("vpc", "vpc_id").unwrap();
    assert_eq!(result.unwrap(), "new-id");
}

#[test]
fn test_clear_outputs() {
    let (_dir, store) = create_test_store();

    store.set_output("vpc", "vpc_id", "vpc-123").unwrap();
    store.set_output("vpc", "subnet_ids", "sub-123").unwrap();

    store.clear_outputs("vpc").unwrap();

    let result = store.get_output("vpc", "vpc_id").unwrap();
    assert!(result.is_none());
    let result = store.get_output("vpc", "subnet_ids").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_get_module_outputs() {
    let (_dir, store) = create_test_store();

    store.set_output("vpc", "vpc_id", "vpc-123").unwrap();
    store.set_output("vpc", "cidr", "10.0.0.0/16").unwrap();
    store.set_output("sg", "sg_id", "sg-456").unwrap();

    let vpc_outputs = store.get_module_outputs("vpc").unwrap();
    assert_eq!(vpc_outputs.len(), 2);

    let sg_outputs = store.get_module_outputs("sg").unwrap();
    assert_eq!(sg_outputs.len(), 1);
}

#[test]
fn test_run_lifecycle() {
    let (_dir, store) = create_test_store();

    let run_id = store.start_run(5).unwrap();
    assert!(!run_id.is_empty());

    let run = store.get_latest_run().unwrap().unwrap();
    assert_eq!(run.id, run_id);
    assert_eq!(run.status, "running");
    assert_eq!(run.modules_planned, 5);

    store.complete_run(&run_id, "succeeded", 5).unwrap();
    let run = store.get_latest_run().unwrap().unwrap();
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.modules_applied, 5);
    assert!(run.completed_at.is_some());
}

#[test]
fn test_module_status_sets_last_apply_at_on_success() {
    let (_dir, store) = create_test_store();

    store.update_module_status("vpc", "succeeded").unwrap();
    let modules = store.list_modules().unwrap();
    let vpc = modules.iter().find(|m| m.name == "vpc").unwrap();
    assert!(vpc.last_apply_at.is_some());
}

#[test]
fn test_module_status_no_last_apply_on_failure() {
    let (_dir, store) = create_test_store();

    store.update_module_status("vpc", "failed").unwrap();
    let modules = store.list_modules().unwrap();
    let vpc = modules.iter().find(|m| m.name == "vpc").unwrap();
    assert!(vpc.last_apply_at.is_none());
}
