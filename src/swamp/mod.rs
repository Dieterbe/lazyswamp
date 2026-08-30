mod data;
mod model;
mod process;

pub use data::{DataArtifact, DataContent, DataVersion};
pub use model::{MethodSpec, ModelDetails, ModelSummary, TypeDescription, TypeName};
pub use process::{RunEvent, SwampCli};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::Result;

#[async_trait]
pub trait SwampClient: Send + Sync {
    async fn version(&self) -> Result<String>;
    async fn models(&self) -> Result<Vec<ModelSummary>>;
    async fn model(&self, name: &str) -> Result<ModelDetails>;
    async fn describe_type(&self, model_type: &str) -> Result<TypeDescription>;
    async fn validate_method(&self, model: &str, method: &str) -> Result<()>;
    async fn run_method(
        &self,
        model: &str,
        method: &str,
        input: &Value,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>>;
    async fn cancel_method(&self, model: &str) -> Result<()>;
    async fn all_data(&self) -> Result<Vec<DataArtifact>>;
    async fn data(&self, model: &str) -> Result<Vec<DataArtifact>>;
    async fn latest_data(&self, model: &str, name: &str) -> Result<DataContent>;
    async fn data_versions(&self, model: &str, name: &str) -> Result<Vec<DataVersion>>;
    async fn data_version(&self, model: &str, name: &str, version: u64) -> Result<DataContent>;
}
