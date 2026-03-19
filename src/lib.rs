mod act_client;
mod mapping;

wit_bindgen::generate!({
    path: "wit",
    world: "component-world",
    generate_all,
});

use act::core::types::*;
use act_types::cbor;

// WASM custom sections for component metadata.
// SAFETY: link_section places data in named WASM custom sections; no executable code.
#[unsafe(link_section = "act:component")]
#[used]
static _ACT_COMPONENT: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/act_component.cbor")).len()] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/act_component.cbor"));

#[unsafe(link_section = "version")]
#[used]
static _VERSION: [u8; 5] = *b"0.1.0";

#[unsafe(link_section = "description")]
#[used]
static _DESCRIPTION: [u8; 59] = *b"Proxies a remote ACT-HTTP server's tools as local ACT tools";

struct ActHttpBridge;

export!(ActHttpBridge);

/// Helper: create a response stream from events.
fn respond(events: Vec<StreamEvent>) -> wit_bindgen::rt::async_support::StreamReader<StreamEvent> {
    let (mut writer, reader) = wit_stream::new::<StreamEvent>();
    wit_bindgen::spawn(async move {
        writer.write_all(events).await;
    });
    reader
}

/// Helper: create a ToolError from ActHttpError.
fn to_tool_error(e: &act_client::ActHttpError) -> ToolError {
    ToolError {
        kind: e.kind.clone(),
        message: LocalizedString::Plain(e.message.clone()),
        metadata: vec![],
    }
}

impl exports::act::core::tool_provider::Guest for ActHttpBridge {
    async fn get_metadata_schema(_metadata: Vec<(String, Vec<u8>)>) -> Option<String> {
        let schema = schemars::schema_for!(act_client::Config);
        Some(serde_json::to_string(&schema).unwrap())
    }

    async fn list_tools(metadata: Vec<(String, Vec<u8>)>) -> Result<ListToolsResponse, ToolError> {
        let config =
            act_client::parse_config_from_metadata(&metadata).map_err(|e| to_tool_error(&e))?;

        let response = act_client::list_tools(&config)
            .await
            .map_err(|e| to_tool_error(&e))?;

        let tools: Vec<ToolDefinition> = response
            .tools
            .iter()
            .map(mapping::http_tool_to_wit)
            .collect();

        Ok(ListToolsResponse {
            metadata: vec![],
            tools,
        })
    }

    async fn call_tool(
        call: ToolCall,
    ) -> wit_bindgen::rt::async_support::StreamReader<StreamEvent> {
        let events = match call_tool_inner(call).await {
            Ok(events) => events,
            Err(e) => vec![StreamEvent::Error(to_tool_error(&e))],
        };

        respond(events)
    }
}

async fn call_tool_inner(call: ToolCall) -> Result<Vec<StreamEvent>, act_client::ActHttpError> {
    let config = act_client::parse_config_from_metadata(&call.metadata)?;

    // Decode arguments from dCBOR to JSON
    let arguments: serde_json::Value = if call.arguments.is_empty() {
        serde_json::json!({})
    } else {
        cbor::cbor_to_json(&call.arguments).map_err(|e| {
            act_client::ActHttpError::invalid_args(format!("Failed to decode arguments: {e}"))
        })?
    };

    let response = act_client::call_tool(&config, &call.name, arguments).await?;

    Ok(mapping::http_response_to_events(&response))
}
