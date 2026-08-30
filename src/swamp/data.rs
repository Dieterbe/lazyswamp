use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataArtifact {
    pub id: String,
    pub name: String,
    pub version: u64,
    #[serde(default)]
    pub content_type: String,
    #[serde(rename = "type", alias = "dataType", default)]
    pub data_type: String,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub lifetime: String,
    #[serde(default)]
    pub owner_type: String,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataContent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model_name: String,
    pub version: u64,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub lifetime: String,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub owner_type: String,
    #[serde(default)]
    pub owner_definition: Option<OwnerDefinition>,
    #[serde(default)]
    pub content: Value,
}

impl DataContent {
    pub fn effective_owner_type(&self) -> &str {
        if self.owner_type.is_empty() {
            self.owner_definition
                .as_ref()
                .map_or("", |owner| owner.owner_type.as_str())
        } else {
            &self.owner_type
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerDefinition {
    #[serde(default)]
    pub owner_type: String,
    #[serde(default)]
    pub owner_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataVersion {
    pub version: u64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub is_latest: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataListResponse {
    #[serde(default)]
    pub groups: Vec<DataGroup>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DataGroup {
    #[serde(rename = "type", default)]
    pub data_type: String,
    #[serde(default)]
    pub items: Vec<DataArtifact>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct VersionsResponse {
    #[serde(default)]
    pub versions: Vec<DataVersion>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QueryResponse {
    #[serde(default)]
    pub results: Vec<DataContent>,
}

#[cfg(test)]
mod tests {
    use super::DataListResponse;

    #[test]
    fn decodes_checked_in_grouped_data_fixture() {
        let response: DataListResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/data-list.json")).unwrap();
        assert_eq!(response.groups[0].data_type, "resource");
        assert_eq!(response.groups[0].items[0].name, "result");
        assert_eq!(response.groups[0].items[0].version, 2);
    }
}
