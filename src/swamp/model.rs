use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub model_type: String,
    #[serde(default)]
    pub global_arguments_schema: Option<Value>,
    #[serde(default)]
    pub methods: Vec<MethodSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetails {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub model_type: String,
    #[serde(default)]
    pub version: Option<u64>,
    #[serde(default)]
    pub type_version: Option<String>,
    #[serde(default)]
    pub tags: Value,
    #[serde(default)]
    pub global_arguments: Value,
    #[serde(default)]
    pub methods: Vec<MethodSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MethodSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_object")]
    pub arguments: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDescription {
    #[serde(rename = "type")]
    pub model_type: TypeName,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub methods: Vec<MethodSpec>,
    #[serde(default)]
    pub data_output_specs: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypeName {
    pub raw: String,
    pub normalized: String,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

pub fn decode_search_response(value: Value) -> serde_json::Result<Vec<ModelSummary>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        results: Vec<ModelSummary>,
    }

    serde_json::from_value::<Response>(value).map(|response| response.results)
}

#[cfg(test)]
mod tests {
    use super::{ModelSummary, decode_search_response};

    #[test]
    fn decodes_checked_in_search_fixture() {
        #[derive(serde::Deserialize)]
        struct Response {
            results: Vec<ModelSummary>,
        }

        let response: Response =
            serde_json::from_str(include_str!("../../tests/fixtures/model-search.json")).unwrap();
        assert_eq!(response.results[0].name, "hello-world");
        assert_eq!(response.results[0].model_type, "command/shell");
    }

    #[test]
    fn decodes_enriched_search_metadata() {
        let models = decode_search_response(serde_json::json!({
            "results": [{
                "id": "id",
                "name": "shell",
                "type": "command/shell",
                "methods": [{"name": "execute", "arguments": {"type": "object"}}],
                "globalArgumentsSchema": {"type": "object"}
            }]
        }))
        .unwrap();
        assert_eq!(models[0].methods[0].name, "execute");
        assert!(models[0].global_arguments_schema.is_some());
    }
}
