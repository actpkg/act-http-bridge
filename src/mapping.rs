// ACT-HTTP <-> ACT WIT type conversion utilities.

use crate::act::core::types::{ContentPart, LocalizedString, ToolDefinition, ToolError, ToolEvent};
use act_types::cbor::to_cbor;
use act_types::http;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Convert an ACT-HTTP `ToolDefinition` to a WIT `ToolDefinition`.
pub fn http_tool_to_wit(tool: &http::ToolDefinition) -> ToolDefinition {
    let parameters_schema = serde_json::to_string(&tool.parameters_schema)
        .unwrap_or_else(|_| r#"{"type":"object"}"#.to_string());

    let metadata = metadata_json_to_cbor(tool.metadata.as_ref());

    ToolDefinition {
        name: tool.name.clone(),
        description: LocalizedString::Plain(tool.description.clone()),
        parameters_schema,
        metadata,
    }
}

/// Convert an ACT-HTTP `ToolCallResponse` to a list of WIT `ToolEvent`s.
pub fn http_response_to_events(response: &http::ToolCallResponse) -> Vec<ToolEvent> {
    response
        .content
        .iter()
        .map(|part| {
            let data = content_data_to_bytes(&part.data, part.mime_type.as_deref());
            let metadata = metadata_json_to_cbor(part.metadata.as_ref());
            ToolEvent::Content(ContentPart {
                data,
                mime_type: part.mime_type.clone(),
                metadata,
            })
        })
        .collect()
}

/// Convert an ACT-HTTP `ToolError` to a WIT `ToolError`.
#[cfg_attr(not(test), expect(dead_code))]
pub fn http_error_to_wit(error: &http::ToolError) -> ToolError {
    let metadata = metadata_json_to_cbor(error.metadata.as_ref());
    ToolError {
        kind: error.kind.clone(),
        message: LocalizedString::Plain(error.message.clone()),
        metadata,
    }
}

/// Convert content data from JSON value to bytes.
///
/// Per ACT spec: text/* → UTF-8 string bytes, otherwise → base64-decoded or raw JSON bytes.
fn content_data_to_bytes(data: &serde_json::Value, mime_type: Option<&str>) -> Vec<u8> {
    let is_text = mime_type.is_some_and(|m| m.starts_with("text/"));

    match data {
        serde_json::Value::String(s) => {
            if is_text {
                s.as_bytes().to_vec()
            } else {
                // Try base64 decode for non-text types
                BASE64.decode(s).unwrap_or_else(|_| s.as_bytes().to_vec())
            }
        }
        _ => serde_json::to_vec(data).unwrap_or_default(),
    }
}

/// Convert JSON metadata object to WIT metadata (list of CBOR-encoded key-value pairs).
fn metadata_json_to_cbor(metadata: Option<&serde_json::Value>) -> Vec<(String, Vec<u8>)> {
    let Some(serde_json::Value::Object(map)) = metadata else {
        return vec![];
    };
    map.iter().map(|(k, v)| (k.clone(), to_cbor(v))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_definition_mapping() {
        let http_tool = http::ToolDefinition {
            name: "my-tool".to_string(),
            description: "A test tool".to_string(),
            parameters_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
            metadata: None,
        };
        let wit_tool = http_tool_to_wit(&http_tool);
        assert_eq!(wit_tool.name, "my-tool");
        assert!(
            matches!(wit_tool.description, LocalizedString::Plain(ref s) if s == "A test tool")
        );
        assert!(wit_tool.parameters_schema.contains("\"type\":\"object\""));
        assert!(wit_tool.metadata.is_empty());
    }

    #[test]
    fn tool_with_metadata() {
        let http_tool = http::ToolDefinition {
            name: "read-tool".to_string(),
            description: "Reads stuff".to_string(),
            parameters_schema: json!({"type": "object"}),
            metadata: Some(json!({"std:read-only": true})),
        };
        let wit_tool = http_tool_to_wit(&http_tool);
        assert_eq!(wit_tool.metadata.len(), 1);
        assert_eq!(wit_tool.metadata[0].0, "std:read-only");
    }

    #[test]
    fn text_content_to_bytes() {
        let data = json!("Hello, world!");
        let bytes = content_data_to_bytes(&data, Some("text/plain"));
        assert_eq!(bytes, b"Hello, world!");
    }

    #[test]
    fn binary_content_base64() {
        let encoded = BASE64.encode(b"\x89PNG");
        let data = json!(encoded);
        let bytes = content_data_to_bytes(&data, Some("image/png"));
        assert_eq!(bytes, b"\x89PNG");
    }

    #[test]
    fn response_to_events() {
        let response = http::ToolCallResponse {
            content: vec![http::ContentPart {
                data: json!("result text"),
                mime_type: Some("text/plain".to_string()),
                metadata: None,
            }],
            metadata: None,
        };
        let events = http_response_to_events(&response);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ToolEvent::Content(cp) => {
                assert_eq!(cp.data, b"result text");
                assert_eq!(cp.mime_type.as_deref(), Some("text/plain"));
            }
            _ => panic!("expected content event"),
        }
    }

    #[test]
    fn error_mapping() {
        let http_err = http::ToolError {
            kind: "std:not-found".to_string(),
            message: "Tool not found".to_string(),
            metadata: None,
        };
        let wit_err = http_error_to_wit(&http_err);
        assert_eq!(wit_err.kind, "std:not-found");
        assert!(matches!(wit_err.message, LocalizedString::Plain(ref s) if s == "Tool not found"));
    }
}
