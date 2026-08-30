use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub job_count: usize,
    #[serde(default)]
    pub has_inputs: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<u64>,
    #[serde(default)]
    pub tags: Value,
    #[serde(default)]
    pub jobs: Vec<WorkflowJob>,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowJob {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<WorkflowJobDependency>,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowJobDependency {
    pub job: String,
    #[serde(default)]
    pub condition: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStep {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<WorkflowStepDependency>,
    pub task: WorkflowTask,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowStepDependency {
    pub step: String,
    #[serde(default)]
    pub condition: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowTask {
    #[serde(rename = "type")]
    pub task_type: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkflowNode<'a> {
    pub job: &'a WorkflowJob,
    pub step: &'a WorkflowStep,
    pub layer: usize,
}

impl WorkflowDefinition {
    pub fn nodes(&self) -> Vec<WorkflowNode<'_>> {
        let internal_layers: Vec<Vec<usize>> = self
            .jobs
            .iter()
            .map(|job| step_layers(&job.steps))
            .collect();
        let widths: Vec<usize> = internal_layers
            .iter()
            .map(|layers| layers.iter().copied().max().unwrap_or(0) + 1)
            .collect();
        let job_indices: HashMap<&str, usize> = self
            .jobs
            .iter()
            .enumerate()
            .map(|(index, job)| (job.name.as_str(), index))
            .collect();
        let mut starts = vec![0; self.jobs.len()];
        for _ in 0..self.jobs.len() {
            for (index, job) in self.jobs.iter().enumerate() {
                starts[index] = job
                    .depends_on
                    .iter()
                    .filter_map(|dependency| job_indices.get(dependency.job.as_str()))
                    .map(|dependency| starts[*dependency] + widths[*dependency])
                    .max()
                    .unwrap_or(0);
            }
        }

        self.jobs
            .iter()
            .enumerate()
            .flat_map(|(job_index, job)| {
                let start = starts[job_index];
                internal_layers[job_index]
                    .iter()
                    .enumerate()
                    .map(move |(step_index, layer)| WorkflowNode {
                        job,
                        step: &job.steps[step_index],
                        layer: start + layer,
                    })
            })
            .collect()
    }
}

fn step_layers(steps: &[WorkflowStep]) -> Vec<usize> {
    let indices: HashMap<&str, usize> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| (step.name.as_str(), index))
        .collect();
    let mut layers = vec![0; steps.len()];
    for _ in 0..steps.len() {
        for (index, step) in steps.iter().enumerate() {
            layers[index] = step
                .depends_on
                .iter()
                .filter_map(|dependency| indices.get(dependency.step.as_str()))
                .map(|dependency| layers[*dependency] + 1)
                .max()
                .unwrap_or(0);
        }
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::WorkflowDefinition;

    #[test]
    fn decodes_and_layers_checked_in_workflow_fixture() {
        let workflow: WorkflowDefinition =
            serde_json::from_str(include_str!("../../tests/fixtures/workflow-get.json")).unwrap();
        let nodes = workflow.nodes();

        assert_eq!(workflow.name, "update-weight-dashboard");
        assert_eq!(nodes.len(), 5);
        assert_eq!(nodes[0].layer, 0);
        assert_eq!(nodes[3].layer, 1);
        assert_eq!(nodes[4].layer, 2);
    }
}
