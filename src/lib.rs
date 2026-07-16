//! act-http-bridge — proxy a remote ACT-HTTP host as local ACT tools.
//!
//! Sessions live on the bridge: each session holds the upstream URL +
//! headers (and optionally an upstream session-id for cascade close).
//! Per ACT-SESSIONS, callers obtain a session-id via `open-session` and
//! reference it in `std:session-id` metadata on subsequent calls.

#![allow(clippy::all)]

mod act_client;
mod mapping;

wit_bindgen::generate!({
    path: "wit",
    world: "component-world",
    generate_all,
});

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use act_client::{ActHttpError, Config};
use exports::act::sessions::session_provider as session_exports;
use exports::act::tools::tool_provider as tool_exports;
// In act:tools@0.2.0 the data model moved to a function-free `types`
// interface; `localized-string` lives in act:core. The `tool-provider`
// export module no longer re-exports these, so reference them directly.
use act::core::types::LocalizedString;
use act::tools::types::ToolDefinition;

// ── Session registry (component-scoped) ────────────────────────────────────

struct UpstreamSession {
    config: Config,
    /// Upstream session-id, if the bridge opened one. None = bridge
    /// passes through tool calls without an upstream session
    /// (current default — see open_session).
    upstream_id: Option<String>,
}

thread_local! {
    static SESSIONS: RefCell<HashMap<String, UpstreamSession>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

fn alloc_session_id() -> String {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        format!("act-http_{id}")
    })
}

fn lookup_config(session_id: &str) -> Option<Config> {
    SESSIONS.with(|s| s.borrow().get(session_id).map(|u| u.config.clone()))
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn extract_session_id(metadata: &[(String, Vec<u8>)]) -> Option<String> {
    metadata
        .iter()
        .find(|(k, _)| k == "std:session-id")
        .and_then(|(_, v)| {
            ciborium::from_reader::<serde_json::Value, _>(v.as_slice())
                .ok()
                .and_then(|val| match val {
                    serde_json::Value::String(s) => Some(s),
                    _ => None,
                })
        })
}

fn invalid_args(msg: impl Into<String>) -> tool_exports::Error {
    tool_exports::Error {
        kind: act_types::constants::ERR_INVALID_ARGS.to_string(),
        message: LocalizedString::Plain(msg.into()),
        metadata: vec![],
    }
}

fn session_not_found(session_id: &str) -> tool_exports::Error {
    tool_exports::Error {
        kind: act_types::constants::ERR_SESSION_NOT_FOUND.to_string(),
        message: LocalizedString::Plain(format!("Unknown session-id: {session_id}")),
        metadata: vec![],
    }
}

fn http_to_wit_error(e: &ActHttpError) -> tool_exports::Error {
    tool_exports::Error {
        kind: e.kind.clone(),
        message: LocalizedString::Plain(e.message.clone()),
        metadata: vec![],
    }
}

// ── Component entry point ──────────────────────────────────────────────────

struct ActHttpBridge;

export!(ActHttpBridge);

// ── tool-provider ──────────────────────────────────────────────────────────

impl tool_exports::Guest for ActHttpBridge {
    async fn list_tools(
        metadata: Vec<(String, Vec<u8>)>,
    ) -> Result<tool_exports::ListToolsResponse, tool_exports::Error> {
        let session_id = match extract_session_id(&metadata) {
            Some(id) => id,
            None => {
                return Ok(tool_exports::ListToolsResponse {
                    metadata: vec![],
                    tools: vec![],
                });
            }
        };

        let config = match lookup_config(&session_id) {
            Some(c) => c,
            None => return Err(session_not_found(&session_id)),
        };

        let response = act_client::list_tools(&config)
            .await
            .map_err(|e| http_to_wit_error(&e))?;

        let tools: Vec<ToolDefinition> = response
            .tools
            .iter()
            .map(mapping::http_tool_to_wit)
            .collect();

        Ok(tool_exports::ListToolsResponse {
            metadata: vec![],
            tools,
        })
    }

    async fn call_tool(
        name: String,
        arguments: Vec<u8>,
        metadata: Vec<(String, Vec<u8>)>,
    ) -> tool_exports::ToolResult {
        let session_id = match extract_session_id(&metadata) {
            Some(id) => id,
            None => {
                return tool_exports::ToolResult::Immediate(vec![tool_exports::ToolEvent::Error(
                    invalid_args("Missing required metadata key std:session-id"),
                )]);
            }
        };

        let config = match lookup_config(&session_id) {
            Some(c) => c,
            None => {
                return tool_exports::ToolResult::Immediate(vec![tool_exports::ToolEvent::Error(
                    session_not_found(&session_id),
                )]);
            }
        };

        // Decode arguments from CBOR to JSON
        let args_json: serde_json::Value = if arguments.is_empty() {
            serde_json::json!({})
        } else {
            match act_types::cbor::cbor_to_json(&arguments) {
                Ok(v) => v,
                Err(e) => {
                    return tool_exports::ToolResult::Immediate(vec![
                        tool_exports::ToolEvent::Error(invalid_args(format!(
                            "Failed to decode arguments: {e}"
                        ))),
                    ]);
                }
            }
        };

        let events = match act_client::call_tool(&config, &name, args_json).await {
            Ok(response) => mapping::http_response_to_events(&response),
            Err(e) => vec![tool_exports::ToolEvent::Error(http_to_wit_error(&e))],
        };
        tool_exports::ToolResult::Immediate(events)
    }
}

// ── session-provider ───────────────────────────────────────────────────────

impl session_exports::Guest for ActHttpBridge {
    async fn get_open_session_args_schema(
        _metadata: Vec<(String, Vec<u8>)>,
    ) -> Result<String, session_exports::Error> {
        let schema = schemars::schema_for!(Config);
        serde_json::to_string(&schema).map_err(|e| session_exports::Error {
            kind: act_types::constants::ERR_INTERNAL.to_string(),
            message: LocalizedString::Plain(format!("Schema serialization failed: {e}")),
            metadata: vec![],
        })
    }

    async fn open_session(
        args: Vec<(String, Vec<u8>)>,
        _metadata: Vec<(String, Vec<u8>)>,
    ) -> Result<session_exports::Session, session_exports::Error> {
        // Reshape (key, cbor) pairs into a JSON object and decode Config.
        let mut json_map = serde_json::Map::with_capacity(args.len());
        for (k, v) in &args {
            if let Ok(val) = ciborium::from_reader::<serde_json::Value, _>(v.as_slice()) {
                json_map.insert(k.clone(), val);
            }
        }
        let config: Config =
            serde_json::from_value(serde_json::Value::Object(json_map)).map_err(|e| {
                session_exports::Error {
                    kind: act_types::constants::ERR_INVALID_ARGS.to_string(),
                    message: LocalizedString::Plain(format!("Invalid open-session args: {e}")),
                    metadata: vec![],
                }
            })?;

        let id = alloc_session_id();
        SESSIONS.with(|s| {
            s.borrow_mut().insert(
                id.clone(),
                UpstreamSession {
                    config,
                    upstream_id: None,
                },
            );
        });

        Ok(session_exports::Session {
            id,
            metadata: vec![],
        })
    }

    fn close_session(session_id: String) {
        // Snapshot upstream state, drop registry entry, then best-effort
        // close upstream. Cascade close only fires if open_session
        // populated upstream_id (currently never — placeholder for when
        // we add upstream-session pass-through).
        let upstream = SESSIONS.with(|s| {
            s.borrow_mut()
                .remove(&session_id)
                .and_then(|u| u.upstream_id.map(|id| (u.config, id)))
        });
        if let Some((config, upstream_id)) = upstream {
            // Fire-and-forget — close-session is sync per WIT.
            wit_bindgen::spawn_local(async move {
                act_client::close_upstream_session(&config, &upstream_id).await;
            });
        }
    }
}
