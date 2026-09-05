use std::{cmp::Reverse, path::PathBuf, process::Stdio};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

use super::{
    DataArtifact, DataContent, DataVersion, ModelDetails, ModelSummary, SwampClient,
    TypeDescription, WorkflowDefinition, WorkflowSummary,
    data::{DataListResponse, QueryResponse, VersionsResponse},
    model::decode_search_response,
};
use crate::error::{Error, Result};

const ALL_DATA_SELECT: &str = r#"{"id": id, "name": name, "version": version, "createdAt": createdAt, "modelName": modelName, "modelId": modelId, "dataType": dataType, "contentType": contentType, "lifetime": lifetime, "ownerType": ownerType, "streaming": streaming, "size": size, "tags": tags}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    Log(String),
    Finished {
        success: bool,
        lock_contended: bool,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct SwampCli {
    binary: PathBuf,
    repo_dir: PathBuf,
}

impl SwampCli {
    pub fn new(binary: PathBuf, repo_dir: PathBuf) -> Self {
        Self { binary, repo_dir }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.arg("--no-color");
        command.kill_on_drop(true);
        command
    }

    async fn json(&self, args: &[&str], context: &'static str) -> Result<Value> {
        self.json_command(args, context, true).await
    }

    async fn json_without_repo(&self, args: &[&str], context: &'static str) -> Result<Value> {
        self.json_command(args, context, false).await
    }

    async fn json_command(
        &self,
        args: &[&str],
        context: &'static str,
        include_repo: bool,
    ) -> Result<Value> {
        let mut command = self.command();
        command.args(args);
        if include_repo {
            command.arg("--repo-dir").arg(&self.repo_dir);
        }
        let output = command
            .arg("--json")
            .output()
            .await
            .map_err(|source| Error::Spawn {
                program: self.binary.display().to_string(),
                source,
            })?;

        if !output.status.success() {
            return Err(command_error(output.status, &output.stdout, &output.stderr));
        }

        serde_json::from_slice(&output.stdout).map_err(|source| Error::Json { context, source })
    }
}

#[async_trait]
impl SwampClient for SwampCli {
    async fn version(&self) -> Result<String> {
        let output = self
            .command()
            .arg("--version")
            .output()
            .await
            .map_err(|source| Error::Spawn {
                program: self.binary.display().to_string(),
                source,
            })?;
        if !output.status.success() {
            return Err(command_error(output.status, &output.stdout, &output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    async fn models(&self) -> Result<Vec<ModelSummary>> {
        let value = self.json(&["model", "search"], "model search").await?;
        decode_search_response(value).map_err(|source| Error::Json {
            context: "model search",
            source,
        })
    }

    async fn model(&self, name: &str) -> Result<ModelDetails> {
        let value = self.json(&["model", "get", name], "model details").await?;
        serde_json::from_value(value).map_err(|source| Error::Json {
            context: "model details",
            source,
        })
    }

    async fn describe_type(&self, model_type: &str) -> Result<TypeDescription> {
        let value = self
            .json_without_repo(
                &["model", "type", "describe", model_type],
                "model type description",
            )
            .await?;
        serde_json::from_value(value).map_err(|source| Error::Json {
            context: "model type description",
            source,
        })
    }

    async fn validate_method(&self, model: &str, method: &str) -> Result<()> {
        self.json(
            &["model", "validate", model, "--method", method],
            "model validation",
        )
        .await
        .map(|_| ())
    }

    async fn run_method(
        &self,
        model: &str,
        method: &str,
        input: &Value,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let mut child = self
            .command()
            .args(["model", "method", "run", model, method, "--stdin"])
            .arg("--repo-dir")
            .arg(&self.repo_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Spawn {
                program: self.binary.display().to_string(),
                source,
            })?;

        let payload = serde_json::to_vec(input).map_err(|source| Error::Json {
            context: "method input",
            source,
        })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&payload).await?;
            stdin.shutdown().await?;
        }

        let stdout = child.stdout.take().ok_or(Error::Incomplete("run stdout"))?;
        let stderr = child.stderr.take().ok_or(Error::Incomplete("run stderr"))?;
        let (sender, receiver) = mpsc::unbounded_channel();

        let stdout_sender = sender.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = stdout_sender.send(RunEvent::Log(line));
            }
        });
        let stderr_sender = sender.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = stderr_sender.send(RunEvent::Log(line));
            }
        });
        tokio::spawn(async move {
            match child.wait().await {
                Ok(status) => {
                    let success = status.success();
                    let lock_contended = status.code() == Some(75);
                    let message = if success {
                        "Method completed successfully".to_owned()
                    } else if lock_contended {
                        "The model is locked by another run; retry later".to_owned()
                    } else {
                        format!("Method exited with {status}")
                    };
                    let _ = sender.send(RunEvent::Finished {
                        success,
                        lock_contended,
                        message,
                    });
                }
                Err(error) => {
                    let _ = sender.send(RunEvent::Finished {
                        success: false,
                        lock_contended: false,
                        message: format!("Could not wait for method: {error}"),
                    });
                }
            }
        });

        Ok(receiver)
    }

    async fn cancel_method(&self, model: &str) -> Result<()> {
        self.json(
            &[
                "model",
                "cancel",
                model,
                "--reason",
                "Cancelled from lazyswamp",
            ],
            "model cancellation",
        )
        .await
        .map(|_| ())
    }

    async fn all_data(&self) -> Result<Vec<DataArtifact>> {
        let value = self
            .json(
                &["data", "query", "isLatest", "--select", ALL_DATA_SELECT],
                "all data metadata",
            )
            .await?;
        let response: QueryResponse =
            serde_json::from_value(value).map_err(|source| Error::Json {
                context: "all data metadata",
                source,
            })?;
        let mut artifacts: Vec<DataArtifact> = response
            .results
            .into_iter()
            .map(DataContent::into_artifact)
            .collect();
        artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(artifacts)
    }

    async fn workflows(&self) -> Result<Vec<WorkflowSummary>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            results: Vec<WorkflowSummary>,
        }
        let value = self
            .json(&["workflow", "search"], "workflow search")
            .await?;
        serde_json::from_value::<Response>(value)
            .map(|response| response.results)
            .map_err(|source| Error::Json {
                context: "workflow search",
                source,
            })
    }

    async fn workflow(&self, name: &str) -> Result<WorkflowDefinition> {
        let value = self
            .json(&["workflow", "get", name], "workflow details")
            .await?;
        serde_json::from_value(value).map_err(|source| Error::Json {
            context: "workflow details",
            source,
        })
    }

    async fn data(&self, model: &str) -> Result<Vec<DataArtifact>> {
        let value = self.json(&["data", "list", model], "data list").await?;
        let response: DataListResponse =
            serde_json::from_value(value).map_err(|source| Error::Json {
                context: "data list",
                source,
            })?;
        let mut artifacts = Vec::new();
        for group in response.groups {
            for mut artifact in group.items {
                if artifact.model_id.is_empty() {
                    artifact.model_id.clone_from(&response.model_id);
                }
                if artifact.model_name.is_empty() {
                    artifact.model_name.clone_from(&response.model_name);
                }
                if artifact.data_type.is_empty() {
                    artifact.data_type.clone_from(&group.data_type);
                }
                artifacts.push(artifact);
            }
        }
        artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(artifacts)
    }

    async fn latest_data(&self, model: &str, name: &str) -> Result<DataContent> {
        let value = self
            .json(&["data", "get", model, name], "data content")
            .await?;
        serde_json::from_value(value).map_err(|source| Error::Json {
            context: "data content",
            source,
        })
    }

    async fn data_versions(&self, model: &str, name: &str) -> Result<Vec<DataVersion>> {
        let value = self
            .json(&["data", "versions", model, name], "data versions")
            .await?;
        let mut response: VersionsResponse =
            serde_json::from_value(value).map_err(|source| Error::Json {
                context: "data versions",
                source,
            })?;
        response
            .versions
            .sort_by_key(|version| Reverse(version.version));
        Ok(response.versions)
    }

    async fn data_version(&self, model: &str, name: &str, version: u64) -> Result<DataContent> {
        let predicate = historical_predicate(model, name, version);
        let value = self
            .json(&["data", "query", &predicate], "historical data")
            .await?;
        let response: QueryResponse =
            serde_json::from_value(value).map_err(|source| Error::Json {
                context: "historical data",
                source,
            })?;
        response
            .results
            .into_iter()
            .next()
            .ok_or(Error::VersionNotFound)
    }
}

pub(crate) fn historical_predicate(model: &str, name: &str, version: u64) -> String {
    format!(
        "modelName == {} && name == {} && version == {version}",
        cel_string(model),
        cel_string(name)
    )
}

fn cel_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

fn command_error(status: std::process::ExitStatus, stdout: &[u8], stderr: &[u8]) -> Error {
    #[derive(Deserialize)]
    struct Envelope {
        error: String,
        #[serde(default)]
        code: Option<String>,
    }

    let envelope = serde_json::from_slice::<Envelope>(stdout)
        .or_else(|_| serde_json::from_slice::<Envelope>(stderr))
        .ok();
    let fallback = if stderr.is_empty() { stdout } else { stderr };
    Error::Command {
        status,
        message: envelope
            .as_ref()
            .map(|item| item.error.clone())
            .unwrap_or_else(|| String::from_utf8_lossy(fallback).trim().to_owned()),
        code: envelope.and_then(|item| item.code),
    }
}

#[cfg(test)]
mod tests {
    use super::historical_predicate;

    #[test]
    fn escapes_cel_string_values() {
        assert_eq!(
            historical_predicate("a\"b", "line\nname", 7),
            "modelName == \"a\\\"b\" && name == \"line\\nname\" && version == 7"
        );
    }
}
