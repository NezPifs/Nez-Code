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
mod guards;
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
use std::sync::Arc;
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
    /// Leave out tools that small local context windows cannot afford
    /// (sub-agents and goals), see [`is_heavy_tool`].
    pub lean_tools: bool,
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
            lean_tools: false,
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
    session: Arc<EndpointSession<T>>,
    dialect: ChatDialect,
}

/// Note shown when the model returns nothing, even after the retry.
pub const EMPTY_REPLY_TEXT: &str = "The model returned an empty reply twice, so the task may be \
    unfinished. Ask again, or switch to a larger model if this keeps happening.";

const RECOVERY_PROMPT_WITH_TOOLS: &str = "Your last reply was empty. Continue the task now: \
    reply with a tool call in the JSON format described above, or with your final answer.";
const RECOVERY_PROMPT: &str = "Your last reply was empty. Answer the last message now.";

/// A second request sent once if the first reply is empty.
struct Recovery<T: HttpTransport> {
    session: Arc<EndpointSession<T>>,
    prepared: Prepared,
    headers: HeaderMap,
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
            session: Arc::new(EndpointSession::new(transport, provider, auth)),
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
        if let Some(reply) = prepared.stop_reply.clone() {
            return Ok(canned_reply(reply));
        }
        let recovery = Recovery {
            session: Arc::clone(&self.session),
            prepared: prepare_recovery(request, &options)?,
            headers: extra_headers.clone(),
        };
        let response = send(
            &self.session,
            options.dialect,
            &prepared.body,
            extra_headers,
        )
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
            Some(recovery),
        ));
        Ok(ResponseStream {
            rx_event: rx,
            upstream_request_id,
            interrupt: None,
        })
    }
}

async fn send<T: HttpTransport>(
    session: &EndpointSession<T>,
    dialect: ChatDialect,
    body: &Value,
    extra_headers: HeaderMap,
) -> Result<nez_client::StreamResponse, ApiError> {
    let (path, accept) = match dialect {
        ChatDialect::OpenAi => ("/chat/completions", "text/event-stream"),
        ChatDialect::Ollama => ("/api/chat", "application/x-ndjson"),
    };
    let body = EncodedJsonBody::encode(body)
        .map_err(|e| ApiError::Stream(format!("failed to encode chat request: {e}")))?;
    session
        .stream_encoded_json_with(Method::POST, path, extra_headers, Some(body), |req| {
            req.headers
                .insert(http::header::ACCEPT, HeaderValue::from_static(accept));
        })
        .await
}

/// A response produced without calling the model.
fn canned_reply(text: String) -> ResponseStream {
    let response_id = next_response_id();
    let (tx, rx) = mpsc::channel(4);
    let events = [
        ResponseEvent::Created {
            response_id: Some(response_id.clone()),
        },
        ResponseEvent::OutputItemDone(stream::assistant_message(
            &format!("{response_id}_msg"),
            &text,
        )),
        ResponseEvent::Completed {
            response_id,
            token_usage: None,
            usage_metadata: None,
            end_turn: None,
        },
    ];
    for event in events {
        // The channel has room for every event, so this cannot fail.
        let _ = tx.try_send(Ok(event));
    }
    ResponseStream {
        rx_event: rx,
        upstream_request_id: None,
        interrupt: None,
    }
}

/// The retry for an empty reply: the JSON text protocol, which small models
/// follow more reliably than a native tool template, plus an explicit nudge.
fn prepare_recovery(
    request: &ResponsesApiRequest,
    options: &ChatOptions,
) -> Result<Prepared, ApiError> {
    let options = ChatOptions {
        native_tools: false,
        text_tools: Some(TextToolFormat::Json),
        structured_output: options.dialect == ChatDialect::Ollama,
        ..options.clone()
    };
    let mut prepared = prepare(request, &options)?;
    let prompt = if prepared.text_tools {
        RECOVERY_PROMPT_WITH_TOOLS
    } else {
        RECOVERY_PROMPT
    };
    if let Some(messages) = prepared
        .body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
    {
        messages.push(json!({"role": "user", "content": prompt}));
    }
    Ok(prepared)
}

/// A request ready to send plus what is needed to interpret the reply.
pub(crate) struct Prepared {
    pub body: Value,
    pub tools: convert::ChatTools,
    /// Tool calls arrive as text and are parsed when the reply completes.
    pub text_tools: bool,
    /// Set when the history shows the turn should end without asking the model.
    pub stop_reply: Option<String>,
    /// A reply with only unusable tool blocks asks the model to try again.
    /// Cleared once it has failed that way `MALFORMED_CALL_RETRIES` times.
    pub retry_malformed_calls: bool,
}

/// Prefix of the note that replaces a tool block Nez-Code could not run.
const TOOL_CALL_NOT_RUN: &str = "Tool call not run:";
const MALFORMED_CALL_RETRIES: usize = 2;
const MALFORMED_CALL_NUDGE: &str = "Your last tool call could not be run; the reason is in your \
     previous message. Fix the call and try again, using the tool format from the system prompt.";

/// Builds the provider request body.
pub(crate) fn prepare(
    request: &ResponsesApiRequest,
    options: &ChatOptions,
) -> Result<Prepared, ApiError> {
    let input = serde_json::to_value(&request.input)
        .map_err(|e| ApiError::Stream(format!("failed to encode chat history: {e}")))?;
    let mut input = input.as_array().cloned().unwrap_or_default();
    guards::collapse_duplicate_results(&mut input);
    let reminder = guards::goal_reminder(&input);
    let stop_reply = guards::repeated_failure(&input);

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
    let mut tools = convert::convert_tools(Some(&Value::Array(tool_values)));
    if options.lean_tools {
        tools.retain(|name| !is_heavy_tool(name));
    }
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
    let output_schema = request
        .text
        .as_ref()
        .and_then(|text| text.format.as_ref())
        .map(|format| format.schema.clone());
    if let Some(format) = text_format {
        append_system_text(
            &mut messages,
            &text_tools::protocol_instructions(&tools, format),
        );
        // Tool blocks are plain text here, so constraining the whole reply
        // to the caller's schema would make tools unreachable. Ask for the
        // schema in the final answer instead.
        if let Some(schema) = &output_schema {
            append_system_text(
                &mut messages,
                &format!(
                    "When you give your final answer, write it as JSON that matches this \
                     schema and nothing else:\n{schema}"
                ),
            );
        }
    }

    let malformed_replies = trailing_malformed_replies(&input);
    if text_format.is_some() && malformed_replies > 0 {
        // Ollama continues a trailing assistant message instead of answering it.
        messages.push(json!({"role": "user", "content": MALFORMED_CALL_NUDGE}));
    }
    if let Some(reminder) = reminder {
        messages.push(json!({"role": "user", "content": reminder}));
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
    let schema = if text_format.is_some() {
        // Schema-constrained decoding removes most malformed tool blocks,
        // but vision models handle images poorly under it.
        (options.structured_output && options.dialect == ChatDialect::Ollama && !images_present)
            .then(|| text_tools::structured_output_schema(&tools))
    } else {
        output_schema
    };

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
        stop_reply,
        retry_malformed_calls: malformed_replies < MALFORMED_CALL_RETRIES,
    })
}

/// Sub-agent and goal tools: about 3k tokens of definitions that a 16k local
/// context cannot spare, for workflows small models do not manage well.
pub fn is_heavy_tool(flat_name: &str) -> bool {
    flat_name.starts_with("multi_agent")
        || matches!(flat_name, "get_goal" | "create_goal" | "update_goal")
}

/// Counts the assistant replies at the end of the history that only reported
/// tool calls that could not be run.
fn trailing_malformed_replies(input: &[Value]) -> usize {
    input
        .iter()
        .rev()
        .take_while(|item| {
            item.get("type").and_then(Value::as_str) == Some("message")
                && item.get("role").and_then(Value::as_str) == Some("assistant")
        })
        .filter(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .any(|text| text.contains(TOOL_CALL_NOT_RUN))
        })
        .count()
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

async fn pump<T: HttpTransport + 'static>(
    bytes: nez_client::ByteStream,
    tx: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    mut prepared: Prepared,
    dialect: ChatDialect,
    idle_timeout: Duration,
    mut recovery: Option<Recovery<T>>,
) {
    let response_id = next_response_id();
    let mut acc = stream::Accumulator::new(response_id.clone());
    if tx
        .send(Ok(ResponseEvent::Created {
            response_id: Some(response_id.clone()),
        }))
        .await
        .is_err()
    {
        return;
    }

    let mut bytes = bytes;
    loop {
        if read_reply(bytes, &tx, &mut acc, &prepared, dialect, idle_timeout)
            .await
            .is_err()
        {
            return;
        }
        if acc.has_output() {
            break;
        }
        let Some(retry) = recovery.take() else {
            acc.text = EMPTY_REPLY_TEXT.to_string();
            break;
        };
        tracing::info!("model returned an empty reply; retrying once with the JSON tool protocol");
        match send(&retry.session, dialect, &retry.prepared.body, retry.headers).await {
            Ok(response) => {
                acc.restart();
                prepared = retry.prepared;
                bytes = response.bytes;
            }
            Err(err) => {
                tracing::warn!("empty-reply retry failed: {err}");
                acc.text = EMPTY_REPLY_TEXT.to_string();
                break;
            }
        }
    }

    // The fallback note is plain prose; never parse it as tool calls.
    let text_tools = prepared.text_tools && acc.text != EMPTY_REPLY_TEXT;
    let mut events = finish(&mut acc, &prepared.tools, text_tools);
    if !prepared.retry_malformed_calls {
        for event in &mut events {
            if let ResponseEvent::Completed { end_turn, .. } = event {
                *end_turn = None;
            }
        }
    }
    for event in events {
        if tx.send(Ok(event)).await.is_err() {
            return;
        }
    }
}

/// Streams one reply into `acc`. `Err` means the stream failed (the error was
/// already sent) or the receiver went away.
async fn read_reply(
    mut bytes: nez_client::ByteStream,
    tx: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    acc: &mut stream::Accumulator,
    prepared: &Prepared,
    dialect: ChatDialect,
    idle_timeout: Duration,
) -> Result<(), ()> {
    let mut splitter = stream::LineSplitter::default();
    loop {
        let next = match tokio::time::timeout(idle_timeout, bytes.next()).await {
            Ok(next) => next,
            Err(_) => {
                let _ = tx
                    .send(Err(ApiError::Stream(
                        "model server stopped responding".to_string(),
                    )))
                    .await;
                return Err(());
            }
        };
        let (lines, ended) = match next {
            Some(Ok(chunk)) => (splitter.push(&chunk), false),
            Some(Err(err)) => {
                let _ = tx.send(Err(ApiError::Transport(err))).await;
                return Err(());
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
                            return Err(());
                        }
                    }
                    if acc.done {
                        return Ok(());
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = tx.send(Err(err)).await;
                    return Err(());
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
                return Err(());
            }
            return Ok(());
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
    let mut end_turn = None;
    if text_tools {
        let parsed = text_tools::parse(&acc.text, tools);
        // Only errors: send them back so the model can correct the call
        // instead of ending the turn on them.
        if parsed.calls.is_empty() && !parsed.errors.is_empty() {
            end_turn = Some(false);
        }
        let mut text = parsed.text;
        for error in &parsed.errors {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&format!("{TOOL_CALL_NOT_RUN} {error}"));
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
        end_turn,
    });
    events
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
