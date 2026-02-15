use crate::config::types::YamlConfig;

/// A planned batch of modules to execute.
#[derive(Debug)]
pub struct PlannedBatch {
    pub batch_number: usize,
    pub modules: Vec<PlannedModule>,
}

/// A module within an execution plan.
#[derive(Debug)]
pub struct PlannedModule {
    pub name: String,
    pub source: String,
    pub depends_on: Vec<String>,
}

/// The full execution plan.
#[derive(Debug)]
pub struct ExecutionPlan {
    pub batches: Vec<PlannedBatch>,
    pub total_modules: usize,
}

impl ExecutionPlan {
    /// Build an execution plan from resolved parallel batches.
    pub fn from_batches(config: &YamlConfig, batches: &[Vec<String>]) -> Self {
        let mut planned_batches = Vec::new();
        let mut total = 0;

        for (i, batch) in batches.iter().enumerate() {
            let modules: Vec<PlannedModule> = batch
                .iter()
                .map(|name| {
                    let module_config = &config.project.modules[name];
                    PlannedModule {
                        name: name.clone(),
                        source: module_config.source.clone(),
                        depends_on: module_config.depends_on.clone(),
                    }
                })
                .collect();

            total += modules.len();
            planned_batches.push(PlannedBatch {
                batch_number: i + 1,
                modules,
            });
        }

        ExecutionPlan {
            batches: planned_batches,
            total_modules: total,
        }
    }
}
