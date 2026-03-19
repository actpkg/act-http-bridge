use act_types::http::{
    ErrorResponse, HEADER_PROTOCOL_VERSION, ListToolsResponse, PROTOCOL_VERSION, ToolCallRequest,
    ToolCallResponse,
};
use http::Uri;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;
use wasip3::http::types::{ErrorCode, Fields, Method, Request, RequestOptions, Response, Scheme};

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
    let response_bytes = http_request(config, Method::Post, &url, &body).await?;
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
        http_request_with_status(config, Method::Post, &url, &body).await?;

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
    let uri: Uri = url
        .parse()
        .map_err(|e| ActHttpError::invalid_args(format!("Invalid URL: {e}")))?;

    let scheme = match uri.scheme_str() {
        Some("https") => Scheme::Https,
        Some("http") => Scheme::Http,
        Some(other) => {
            return Err(ActHttpError::invalid_args(format!(
                "Unsupported scheme: {other}"
            )));
        }
        None => return Err(ActHttpError::invalid_args("Missing scheme in URL")),
    };

    // Build headers
    let mut header_list: Vec<(String, Vec<u8>)> = vec![
        ("content-type".to_string(), b"application/json".to_vec()),
        ("accept".to_string(), b"application/json".to_vec()),
        (
            HEADER_PROTOCOL_VERSION.to_lowercase(),
            PROTOCOL_VERSION.as_bytes().to_vec(),
        ),
    ];
    for (key, value) in &config.headers {
        header_list.push((key.to_lowercase(), value.as_bytes().to_vec()));
    }
    let headers = Fields::from_list(&header_list)
        .map_err(|e| ActHttpError::internal(format!("Headers error: {e:?}")))?;

    // Build request body stream
    let body_vec = body_bytes.to_vec();
    let (mut body_writer, body_reader) = wasip3::wit_stream::new::<u8>();
    wit_bindgen::spawn(async move {
        body_writer.write_all(body_vec).await;
    });

    // Trailers (none)
    let (_, trailers_reader) =
        wasip3::wit_future::new::<Result<Option<Fields>, ErrorCode>>(|| Ok(None));

    // Timeout: 30s
    let timeout_ns = 30_000 * 1_000_000u64;
    let opts = RequestOptions::new();
    let _ = opts.set_connect_timeout(Some(timeout_ns));
    let _ = opts.set_first_byte_timeout(Some(timeout_ns));

    // Construct request
    let (request, _) = Request::new(headers, Some(body_reader), trailers_reader, Some(opts));
    let _ = request.set_method(&method);
    let _ = request.set_scheme(Some(&scheme));

    if let Some(authority) = uri.authority() {
        let _ = request.set_authority(Some(authority.as_str()));
    }

    let _ = request.set_path_with_query(uri.path_and_query().map(|pq| pq.as_str()));

    // Send request
    let response = wasip3::http::client::send(request)
        .await
        .map_err(|e| ActHttpError::internal(format!("HTTP error: {e:?}")))?;

    let status = response.get_status_code();

    // Read response body
    let (_, result_reader) = wasip3::wit_future::new::<Result<(), ErrorCode>>(|| Ok(()));
    let (mut body_stream, _trailers) = Response::consume_body(response, result_reader);

    let mut all_bytes = Vec::new();
    let mut read_buf = Vec::with_capacity(16384);
    loop {
        let (result, chunk) = body_stream.read(read_buf).await;
        match result {
            wasip3::wit_bindgen::StreamResult::Complete(_) => {
                all_bytes.extend_from_slice(&chunk);
                if all_bytes.len() > MAX_RESPONSE_BYTES {
                    return Err(ActHttpError::internal("Response too large"));
                }
                read_buf = Vec::with_capacity(16384);
            }
            wasip3::wit_bindgen::StreamResult::Dropped
            | wasip3::wit_bindgen::StreamResult::Cancelled => break,
        }
    }

    Ok((status, all_bytes))
}
