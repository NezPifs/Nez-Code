//! Chat-completions transport for local and OpenAI-compatible providers.
//!
//! Requests are built from the same [`ResponsesApiRequest`] used for the
//! Responses API so the agent loop does not care which wire protocol a
//! provider speaks. Two dialects are supported:
//!
//! - [`ChatDialect::OpenAi`]: `POST {base_url}/chat/completions` with SSE.
//!   Used for DeepSeek and other hosted OpenAI-compatible APIs.
//! - [`ChatDialect::Ollama`]: Ollama's native `POST /api/chat` with NDJSON.
//!   Preferred for local models because it accepts `num_ctx`, `think` and a
//!   JSON-schema `format`, none of which the OpenAI shim exposes.

mod capabilities;
mod convert;
mod stream;
mod text_tools;

use crate::auth::SharedAuthProvider;
use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::common::ResponsesApiRequest;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use futures::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use nez_client::EncodedJsonBody;
use nez_client::HttpTransport;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;

pub use capabilities::ModelCapabilities;
pub use capabilities::ollama_capabilities;
pub use capabilities::ollama_root;
pub use convert::IMAGE_OMITTED_TEXT;
pub use nez_model_profiles::TextToolFormat;

/// Which chat protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatDialect {
    OpenAi,
    Ollama,
}

/// Per-request knobs derived from the model profile and user configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatOptions {
    pub dialect: ChatDialect,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    /// Ollama context length. Ignored by the OpenAI dialect.
    pub num_ctx: Option<u32>,
    /// `Some(false)` disables model thinking. Ollama receives `think`, the
    /// OpenAI dialect receives `reasoning_effort: "none"` when `send_reasoning_effort` is set.
    pub think: Option<bool>,
    /// Whether `reasoning_effort` may be sent to an OpenAI-compatible server.
    /// Ollama's shim honours it; most hosted providers reject unknown values.
    pub send_reasoning_effort: bool,
    /// Send tools as native function definitions.
    pub native_tools: bool,
    /// Describe tools in the system prompt and parse calls out of the reply
    /// instead. Takes precedence over `native_tools`.
    pub text_tools: Option<TextToolFormat>,
    /// Constrain text-protocol replies with a JSON schema. Only the Ollama
    /// dialect supports arbitrary schemas; requests with images skip it.
    pub structured_output: bool,
    /// Forward image content. When false, images are replaced with a note.
    pub allow_images: bool,
    pub max_output_tokens: Option<u32>,
}

impl ChatOptions {
    pub fn new(dialect: ChatDialect) -> Self {
        Self {
            dialect,
            temperature: None,
            top_p: None,
            num_ctx: None,
            think: None,
            send_reasoning_effort: false,
            native_tools: true,
            text_tools: None,
            structured_output: false,
            allow_images: true,
            max_output_tokens: None,
        }
    }
}

/// Monotonic id source for synthesized response ids.
static RESPONSE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_response_id() -> String {
    let n = RESPONSE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("chat_{nanos:x}_{n}")
}

pub struct ChatClient<T: HttpTransport> {
    session: EndpointSession<T>,
    dialect: ChatDialect,
}

impl<T: HttpTransport + 'static> ChatClient<T> {
    /// `provider.base_url` is the OpenAI-style base (`.../v1`); for Ollama the
    /// native API lives at the server root.
    pub fn new(
        transport: T,
        mut provider: Provider,
        auth: SharedAuthProvider,
        dialect: ChatDialect,
    ) -> Self {
        if dialect == ChatDialect::Ollama {
            provider.base_url = ollama_root(&provider.base_url);
        }
        Self {
            session: EndpointSession::new(transport, provider, auth),
            dialect,
        }
    }

    pub async fn stream_request(
        &self,
        request: &ResponsesApiRequest,
        options: ChatOptions,
        extra_headers: HeaderMap,
    ) -> Result<ResponseStream, ApiError> {
        let options = ChatOptions {
            dialect: self.dialect,
            ..options
        };
        let prepared = prepare(request, &options)?;
        let path = match options.dialect {
            ChatDialect::OpenAi => "/chat/completions",
            ChatDialect::Ollama => "/api/chat",
        };
        let body = EncodedJsonBody::encode(&prepared.body)
            .map_err(|e| ApiError::Stream(format!("failed to encode chat request: {e}")))?;
        let accept = match options.dialect {
            ChatDialect::OpenAi => "text/event-stream",
            ChatDialect::Ollama => "application/x-ndjson",
        };
        let response = self
            .session
            .stream_encoded_json_with(Method::POST, path, extra_headers, Some(body), |req| {
                req.headers
                    .insert(http::header::ACCEPT, HeaderValue::from_static(accept));
            })
            .await?;
        let upstream_request_id = response
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let idle_timeout = self.session.provider().stream_idle_timeout;
        let (tx, rx) = mpsc::channel(1600);
        tokio::spawn(pump(
            response.bytes,
            tx,
            prepared,
            options.dialect,
            idle_timeout,
        ));
        Ok(ResponseStream {
            rx_event: rx,
            upstream_request_id,
            interrupt: None,
        })
    }
}

/// A request ready to send plus what is needed to interpret the reply.
pub(crate) struct Prepared {
    pub body: Value,
    pub tools: convert::ChatTools,
    /// Tool calls arrive as text and are parsed when the reply completes.
    pub text_tools: bool,
}

/// Builds the provider request body.
pub(crate) fn prepare(
    request: &ResponsesApiRequest,
    options: &ChatOptions,
) -> Result<Prepared, ApiError> {
    let input = serde_json::to_value(&request.input)
        .map_err(|e| ApiError::Stream(format!("failed to encode chat history: {e}")))?;
    let input = input.as_array().cloned().unwrap_or_default();

    let mut tool_values: Vec<Value> = request
        .tools
        .as_ref()
        .and_then(|tools| serde_json::to_value(tools).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    // Some sessions deliver tools as history items instead of a request field.
    for item in &input {
        if item.get("type").and_then(Value::as_str) == Some("additional_tools")
            && let Some(extra) = item.get("tools").and_then(Value::as_array)
        {
            tool_values.extend(extra.iter().cloned());
        }
    }
    let tools = convert::convert_tools(Some(&Value::Array(tool_values)));
    let text_format = options.text_tools.filter(|_| !tools.is_empty());
    let native_tools = options.native_tools && text_format.is_none();

    let mut messages = convert::convert_input(
        "",
        &input,
        convert::ConvertOptions {
            dialect: options.dialect,
            allow_images: options.allow_images,
            native_tools,
        },
    );
    if let Some(format) = text_format {
        append_system_text(
            &mut messages,
            &text_tools::protocol_instructions(&tools, format),
        );
    }

    let images_present = has_images(&messages);
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(request.model.clone()));
    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), Value::Bool(true));
    let send_tools = native_tools && !tools.is_empty();
    if send_tools {
        body.insert("tools".to_string(), Value::Array(tools.definitions.clone()));
    }
    let schema = request
        .text
        .as_ref()
        .and_then(|text| text.format.as_ref())
        .map(|format| format.schema.clone())
        .or_else(|| {
            // Schema-constrained decoding removes most malformed tool blocks,
            // but vision models handle images poorly under it.
            (text_format.is_some()
                && options.structured_output
                && options.dialect == ChatDialect::Ollama
                && !images_present)
                .then(|| text_tools::structured_output_schema(&tools))
        });

    match options.dialect {
        ChatDialect::OpenAi => {
            body.insert("stream_options".to_string(), json!({"include_usage": true}));
            if send_tools {
                body.insert("tool_choice".to_string(), Value::String("auto".to_string()));
            }
            if let Some(t) = options.temperature {
                body.insert("temperature".to_string(), json!(t));
            }
            if let Some(p) = options.top_p {
                body.insert("top_p".to_string(), json!(p));
            }
            if let Some(max) = options.max_output_tokens {
                body.insert("max_tokens".to_string(), json!(max));
            }
            if options.send_reasoning_effort && options.think == Some(false) {
                body.insert(
                    "reasoning_effort".to_string(),
                    Value::String("none".to_string()),
                );
            }
            if let Some(schema) = schema {
                body.insert(
                    "response_format".to_string(),
                    json!({
                        "type": "json_schema",
                        "json_schema": {"name": "nez_output", "schema": schema},
                    }),
                );
            }
        }
        ChatDialect::Ollama => {
            let mut model_options = Map::new();
            if let Some(t) = options.temperature {
                model_options.insert("temperature".to_string(), json!(t));
            }
            if let Some(p) = options.top_p {
                model_options.insert("top_p".to_string(), json!(p));
            }
            if let Some(ctx) = options.num_ctx {
                model_options.insert("num_ctx".to_string(), json!(ctx));
            }
            if let Some(max) = options.max_output_tokens {
                model_options.insert("num_predict".to_string(), json!(max));
            }
            if !model_options.is_empty() {
                body.insert("options".to_string(), Value::Object(model_options));
            }
            if let Some(think) = options.think {
                body.insert("think".to_string(), Value::Bool(think));
            }
            if let Some(schema) = schema {
                body.insert("format".to_string(), schema);
            }
        }
    }

    Ok(Prepared {
        body: Value::Object(body),
        tools,
        text_tools: text_format.is_some(),
    })
}

/// Appends a paragraph to the leading system message, creating one if needed.
fn append_system_text(messages: &mut Vec<Value>, text: &str) {
    if let Some(first) = messages.first_mut()
        && first.get("role").and_then(Value::as_str) == Some("system")
        && let Some(content) = first.get("content").and_then(Value::as_str)
    {
        let joined = if content.trim().is_empty() {
            text.to_string()
        } else {
            format!("{content}\n\n{text}")
        };
        first["content"] = Value::String(joined);
        return;
    }
    messages.insert(0, json!({"role": "system", "content": text}));
}

fn has_images(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message
            .get("images")
            .and_then(Value::as_array)
            .is_some_and(|images| !images.is_empty())
            || message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("type").and_then(Value::as_str) == Some("image_url"))
                })
    })
}

async fn pump(
    mut bytes: nez_client::ByteStream,
    tx: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    prepared: Prepared,
    dialect: ChatDialect,
    idle_timeout: Duration,
) {
    let response_id = next_response_id();
    let mut acc = stream::Accumulator::new(response_id.clone());
    let mut splitter = stream::LineSplitter::default();
    if tx
        .send(Ok(ResponseEvent::Created {
            response_id: Some(response_id.clone()),
        }))
        .await
        .is_err()
    {
        return;
    }

    'read: loop {
        let next = match tokio::time::timeout(idle_timeout, bytes.next()).await {
            Ok(next) => next,
            Err(_) => {
                let _ = tx
                    .send(Err(ApiError::Stream(
                        "model server stopped responding".to_string(),
                    )))
                    .await;
                return;
            }
        };
        let (lines, ended) = match next {
            Some(Ok(chunk)) => (splitter.push(&chunk), false),
            Some(Err(err)) => {
                let _ = tx.send(Err(ApiError::Transport(err))).await;
                return;
            }
            None => (splitter.finish().into_iter().collect::<Vec<_>>(), true),
        };
        for line in lines {
            match stream::parse_line(dialect, &line) {
                Ok(Some(chunk)) => {
                    // Text-protocol replies are parsed as a whole; streaming
                    // them would show raw tool blocks to the user.
                    for event in acc.push(chunk, !prepared.text_tools) {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    if acc.done {
                        break 'read;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = tx.send(Err(err)).await;
                    return;
                }
            }
        }
        if ended {
            // A connection closed by a proxy or a crashed server looks like a
            // clean end of stream. Without a terminal marker the reply may be
            // cut short, so let the caller retry instead of keeping it.
            if !acc.done && acc.finish_reason.is_none() {
                let _ = tx
                    .send(Err(ApiError::Stream(
                        "model server closed the stream before the reply finished".to_string(),
                    )))
                    .await;
                return;
            }
            break;
        }
    }

    for event in finish(&mut acc, &prepared.tools, prepared.text_tools) {
        if tx.send(Ok(event)).await.is_err() {
            return;
        }
    }
}

/// Produces the closing events for a finished response.
pub(crate) fn finish(
    acc: &mut stream::Accumulator,
    tools: &convert::ChatTools,
    text_tools: bool,
) -> Vec<ResponseEvent> {
    let mut events = Vec::new();
    let response_id = acc.message_id().trim_end_matches("_msg").to_string();
    if !acc.reasoning.trim().is_empty()
        && let Some(item) = stream::reasoning_item(&format!("{response_id}_rs"), &acc.reasoning)
    {
        events.push(ResponseEvent::OutputItemDone(item));
    }
    if text_tools {
        let parsed = text_tools::parse(&acc.text, tools);
        let mut text = parsed.text;
        for error in &parsed.errors {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&format!("Tool call not run: {error}"));
        }
        if !text.is_empty() {
            events.push(ResponseEvent::OutputItemDone(stream::assistant_message(
                &acc.message_id(),
                &text,
            )));
        }
        for (index, call) in parsed.calls.into_iter().enumerate() {
            let call_id = format!("{response_id}_text_call_{index}");
            if let Some(item) = stream::tool_call_item(tools, &call.name, call.arguments, call_id) {
                events.push(ResponseEvent::OutputItemDone(item));
            }
        }
    } else if !acc.text.is_empty() || acc.message_started() {
        events.push(ResponseEvent::OutputItemDone(stream::assistant_message(
            &acc.message_id(),
            &acc.text,
        )));
    }
    // Some servers still return structured calls in text mode; run those too.
    for item in acc.take_tool_calls(tools) {
        events.push(ResponseEvent::OutputItemDone(item));
    }
    events.push(ResponseEvent::Completed {
        response_id,
        token_usage: acc.usage.clone(),
        usage_metadata: None,
        end_turn: None,
    });
    events
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
