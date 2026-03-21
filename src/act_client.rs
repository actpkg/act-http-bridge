use act_types::http::{
    ErrorResponse, HEADER_PROTOCOL_VERSION, ListToolsResponse, PROTOCOL_VERSION, ToolCallRequest,
    ToolCallResponse,
};
use http::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize, JsonSchema)]
pub struct Config {
    /// Base URL of the remote ACT-HTTP server (e.g. http://localhost:3000)
    pub url: String,
    /// Optional default headers sent with every request (e.g. authorization)
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct ActHttpError {
    pub kind: String,
    pub message: String,
}

impl ActHttpError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            kind: "std:internal".to_string(),
            message: msg.into(),
        }
    }

    pub fn invalid_args(msg: impl Into<String>) -> Self {
        Self {
            kind: "std:invalid-args".to_string(),
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for ActHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/// Extract Config from metadata key-value pairs.
/// Each value is CBOR-encoded.
pub fn parse_config_from_metadata(metadata: &[(String, Vec<u8>)]) -> Result<Config, ActHttpError> {
    let url = metadata
        .iter()
        .find(|(k, _)| k == "url")
        .map(|(_, v)| act_types::cbor::from_cbor::<String>(v))
        .transpose()
        .map_err(|e| ActHttpError::invalid_args(format!("Invalid url in metadata: {e}")))?
        .ok_or_else(|| ActHttpError::invalid_args("Missing 'url' in metadata"))?;

    let headers = metadata
        .iter()
        .find(|(k, _)| k == "headers")
        .map(|(_, v)| act_types::cbor::from_cbor::<BTreeMap<String, String>>(v))
        .transpose()
        .map_err(|e| ActHttpError::invalid_args(format!("Invalid headers in metadata: {e}")))?
        .unwrap_or_default();

    Ok(Config { url, headers })
}

/// Fetch tool definitions from a remote ACT-HTTP server.
pub async fn list_tools(config: &Config) -> Result<ListToolsResponse, ActHttpError> {
    let url = format!("{}/tools", config.url.trim_end_matches('/'));
    let body = serde_json::to_vec(&serde_json::json!({}))
        .map_err(|e| ActHttpError::internal(format!("JSON serialize error: {e}")))?;
    let response_bytes = http_request(config, Method::POST, &url, &body).await?;
    serde_json::from_slice(&response_bytes)
        .map_err(|e| ActHttpError::internal(format!("Invalid tools response: {e}")))
}

/// Call a tool on a remote ACT-HTTP server.
pub async fn call_tool(
    config: &Config,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<ToolCallResponse, ActHttpError> {
    let url = format!("{}/tools/{}", config.url.trim_end_matches('/'), tool_name);
    let request = ToolCallRequest {
        arguments,
        metadata: None,
    };
    let body = serde_json::to_vec(&request)
        .map_err(|e| ActHttpError::internal(format!("JSON serialize error: {e}")))?;
    let (status, response_bytes) =
        http_request_with_status(config, Method::POST, &url, &body).await?;

    if !(200..300).contains(&status) {
        // Try to parse as ACT error response
        if let Ok(err_resp) = serde_json::from_slice::<ErrorResponse>(&response_bytes) {
            return Err(ActHttpError {
                kind: err_resp.error.kind,
                message: err_resp.error.message,
            });
        }
        // Fallback: map HTTP status to error kind
        let kind = status_to_error_kind(status);
        let detail = String::from_utf8_lossy(&response_bytes);
        return Err(ActHttpError {
            kind: kind.to_string(),
            message: format!("HTTP {status}: {detail}"),
        });
    }

    serde_json::from_slice(&response_bytes)
        .map_err(|e| ActHttpError::internal(format!("Invalid tool response: {e}")))
}

fn status_to_error_kind(status: u16) -> &'static str {
    match status {
        404 => "std:not-found",
        422 => "std:invalid-args",
        408 | 504 => "std:timeout",
        403 => "std:capability-denied",
        _ => "std:internal",
    }
}

const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// HTTP request returning only the body bytes (errors on non-2xx).
async fn http_request(
    config: &Config,
    method: Method,
    url: &str,
    body_bytes: &[u8],
) -> Result<Vec<u8>, ActHttpError> {
    let (status, bytes) = http_request_with_status(config, method, url, body_bytes).await?;
    if !(200..300).contains(&status) {
        let detail = String::from_utf8_lossy(&bytes);
        return Err(ActHttpError::internal(format!("HTTP {status}: {detail}")));
    }
    Ok(bytes)
}

/// HTTP request returning status code and body bytes.
async fn http_request_with_status(
    config: &Config,
    method: Method,
    url: &str,
    body_bytes: &[u8],
) -> Result<(u16, Vec<u8>), ActHttpError> {
    let mut builder = wasi_fetch::Client::new()
        .request(method, url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header(HEADER_PROTOCOL_VERSION, PROTOCOL_VERSION)
        .body(body_bytes.to_vec())
        .timeout(std::time::Duration::from_secs(30))
        .redirect_limit(0);

    for (key, value) in &config.headers {
        builder = builder.header(key.as_str(), value.as_str());
    }

    let response = builder
        .send()
        .await
        .map_err(|e| ActHttpError::internal(format!("HTTP error: {e}")))?;

    let status = response.status().as_u16();
    let body = response.into_body().bytes().await;

    if body.len() > MAX_RESPONSE_BYTES {
        return Err(ActHttpError::internal("Response too large"));
    }

    Ok((status, body.to_vec()))
}
