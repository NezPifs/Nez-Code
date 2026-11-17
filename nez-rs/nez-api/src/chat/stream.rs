//! Decoding of streamed chat output into Nez response events.
//!
//! Both supported dialects are reduced to the same [`ChatChunk`] shape:
//! OpenAI-compatible servers stream server-sent events with `choices[].delta`,
//! Ollama's native API streams newline-delimited JSON objects with a `message`.

use super::ChatDialect;
use super::convert::ChatTools;
use super::convert::ToolKind;
use crate::common::ResponseEvent;
use crate::error::ApiError;
use nez_protocol::models::ResponseItem;
use nez_protocol::protocol::TokenUsage;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

/// One decoded unit of streamed output.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ChatChunk {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCallDelta>,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
    pub done: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ToolCallDelta {
    /// Position of the call in the response. OpenAI streams fragments keyed by
    /// index; Ollama sends complete calls, so the position is used instead.
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// Splits a byte stream into complete lines, tolerating chunk boundaries that
/// fall inside a line or a UTF-8 sequence.
#[derive(Debug, Default)]
pub(crate) struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            lines.push(text.trim_end_matches(['\r', '\n']).to_string());
        }
        lines
    }

    pub(crate) fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let text = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).to_string();
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

/// Parses one line of streamed output. Returns `Ok(None)` for keep-alives and
/// lines that carry no payload.
pub(crate) fn parse_line(dialect: ChatDialect, line: &str) -> Result<Option<ChatChunk>, ApiError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    match dialect {
        ChatDialect::OpenAi => {
            let Some(data) = line.strip_prefix("data:") else {
                // `event:`, `id:` and `:` comment lines carry nothing we use.
                return Ok(None);
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(Some(ChatChunk {
                    done: true,
                    ..Default::default()
                }));
            }
            let value: Value = serde_json::from_str(data)
                .map_err(|err| ApiError::Stream(format!("invalid chat stream chunk: {err}")))?;
            parse_openai_value(&value).map(Some)
        }
        ChatDialect::Ollama => {
            let value: Value = serde_json::from_str(line)
                .map_err(|err| ApiError::Stream(format!("invalid Ollama stream chunk: {err}")))?;
            parse_ollama_value(&value).map(Some)
        }
    }
}

/// Parses a complete (non-streamed) response body.
#[cfg(test)]
pub(crate) fn parse_body(dialect: ChatDialect, body: &str) -> Result<ChatChunk, ApiError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|err| ApiError::Stream(format!("invalid chat response: {err}")))?;
    let mut chunk = match dialect {
        ChatDialect::OpenAi => parse_openai_value(&value)?,
        ChatDialect::Ollama => parse_ollama_value(&value)?,
    };
    chunk.done = true;
    Ok(chunk)
}

fn error_message(value: &Value) -> Option<String> {
    match value.get("error")? {
        Value::String(message) => Some(message.clone()),
        Value::Object(obj) => Some(
            obj.get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
        ),
        other => Some(other.to_string()),
    }
}

fn parse_openai_value(value: &Value) -> Result<ChatChunk, ApiError> {
    if let Some(message) = error_message(value) {
        return Err(ApiError::Stream(message));
    }
    let mut chunk = ChatChunk::default();
    if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
        chunk.usage = Some(openai_usage(usage));
    }
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
    else {
        return Ok(chunk);
    };
    // Streaming chunks use `delta`, complete responses use `message`.
    let message = choice.get("delta").or_else(|| choice.get("message"));
    if let Some(message) = message {
        chunk.content = message
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        chunk.reasoning = message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        for (position, call) in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let function = call.get("function");
            chunk.tool_calls.push(ToolCallDelta {
                index: call
                    .get("index")
                    .and_then(Value::as_u64)
                    .map_or(position, |i| i as usize),
                id: call.get("id").and_then(Value::as_str).map(str::to_string),
                name: function
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                arguments: function.and_then(|f| f.get("arguments")).map(|a| match a {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }),
            });
        }
    }
    chunk.finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(chunk)
}

fn openai_usage(usage: &Value) -> TokenUsage {
    let int = |v: Option<&Value>| v.and_then(Value::as_i64).unwrap_or(0);
    let input = int(usage.get("prompt_tokens"));
    let output = int(usage.get("completion_tokens"));
    let cached = int(usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens")))
    .max(int(usage.get("prompt_cache_hit_tokens")));
    let reasoning = int(usage
        .get("completion_tokens_details")
        .and_then(|d| d.get("reasoning_tokens")));
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input + output);
    TokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        total_tokens: total,
        ..Default::default()
    }
}

fn parse_ollama_value(value: &Value) -> Result<ChatChunk, ApiError> {
    if let Some(message) = error_message(value) {
        return Err(ApiError::Stream(message));
    }
    let mut chunk = ChatChunk::default();
    if let Some(message) = value.get("message") {
        chunk.content = message
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        chunk.reasoning = message
            .get("thinking")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function");
            chunk.tool_calls.push(ToolCallDelta {
                // Ollama sends whole calls; the accumulator assigns positions.
                index: usize::MAX,
                id: call.get("id").and_then(Value::as_str).map(str::to_string),
                name: function
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                arguments: function.and_then(|f| f.get("arguments")).map(|a| match a {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }),
            });
        }
    }
    if value.get("done").and_then(Value::as_bool) == Some(true) {
        chunk.done = true;
        chunk.finish_reason = value
            .get("done_reason")
            .and_then(Value::as_str)
            .map(str::to_string);
        let input = value
            .get("prompt_eval_count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let output = value.get("eval_count").and_then(Value::as_i64).unwrap_or(0);
        if input > 0 || output > 0 {
            chunk.usage = Some(TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: input + output,
                ..Default::default()
            });
        }
    }
    Ok(chunk)
}

#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

/// Collects streamed chunks and produces the final response items.
#[derive(Debug, Default)]
pub(crate) struct Accumulator {
    response_id: String,
    pub text: String,
    pub reasoning: String,
    calls: BTreeMap<usize, PartialCall>,
    next_whole_call: usize,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
    message_started: bool,
    pub done: bool,
}

impl Accumulator {
    pub(crate) fn new(response_id: String) -> Self {
        Self {
            response_id,
            ..Default::default()
        }
    }

    pub(crate) fn message_id(&self) -> String {
        format!("{}_msg", self.response_id)
    }

    /// Folds a chunk in. When `stream_text` is set, returns the events that
    /// should be forwarded to the UI immediately.
    pub(crate) fn push(&mut self, chunk: ChatChunk, stream_text: bool) -> Vec<ResponseEvent> {
        let mut events = Vec::new();
        if let Some(reasoning) = chunk.reasoning {
            self.reasoning.push_str(&reasoning);
        }
        if let Some(content) = chunk.content {
            if stream_text {
                if !self.message_started {
                    self.message_started = true;
                    events.push(ResponseEvent::OutputItemAdded(assistant_message(
                        &self.message_id(),
                        "",
                    )));
                }
                events.push(ResponseEvent::OutputTextDelta(content.clone()));
            }
            self.text.push_str(&content);
        }
        for delta in chunk.tool_calls {
            let index = if delta.index == usize::MAX {
                let index = self.next_whole_call;
                self.next_whole_call += 1;
                index
            } else {
                self.next_whole_call = self.next_whole_call.max(delta.index + 1);
                delta.index
            };
            let entry = self.calls.entry(index).or_default();
            if let Some(id) = delta.id.filter(|id| !id.is_empty()) {
                entry.id = Some(id);
            }
            if let Some(name) = delta.name {
                entry.name.push_str(&name);
            }
            if let Some(arguments) = delta.arguments {
                entry.arguments.push_str(&arguments);
            }
        }
        if chunk.usage.is_some() {
            self.usage = chunk.usage;
        }
        if chunk.finish_reason.is_some() {
            self.finish_reason = chunk.finish_reason;
        }
        if chunk.done {
            self.done = true;
        }
        events
    }

    /// Drains native tool calls as response items.
    pub(crate) fn take_tool_calls(&mut self, tools: &ChatTools) -> Vec<ResponseItem> {
        let calls = std::mem::take(&mut self.calls);
        calls
            .into_iter()
            .filter(|(_, call)| !call.name.is_empty())
            .filter_map(|(index, call)| {
                let call_id = call
                    .id
                    .unwrap_or_else(|| format!("{}_call_{index}", self.response_id));
                let arguments = if call.arguments.trim().is_empty() {
                    "{}".to_string()
                } else {
                    call.arguments
                };
                tool_call_item(tools, &call.name, arguments, call_id)
            })
            .collect()
    }

    pub(crate) fn message_started(&self) -> bool {
        self.message_started
    }

    /// Whether the reply produced anything the agent loop can act on.
    /// Reasoning alone does not count: the model thought but never answered.
    pub(crate) fn has_output(&self) -> bool {
        !self.text.trim().is_empty() || self.calls.values().any(|call| !call.name.is_empty())
    }

    /// Clears the reply before a retry. Usage and the started message are
    /// kept so the UI sees one continuous response.
    pub(crate) fn restart(&mut self) {
        self.text.clear();
        self.reasoning.clear();
        self.calls.clear();
        self.next_whole_call = 0;
        self.finish_reason = None;
        self.done = false;
    }
}

/// Builds the response item for one tool call, restoring namespaces and
/// freeform tools that were flattened for the chat request.
pub(crate) fn tool_call_item(
    tools: &ChatTools,
    flat_name: &str,
    arguments: String,
    call_id: String,
) -> Option<ResponseItem> {
    let value = match tools.kind(flat_name) {
        ToolKind::Function { name, namespace } => json!({
            "type": "function_call",
            "name": name,
            "namespace": namespace,
            "arguments": arguments,
            "call_id": call_id,
        }),
        ToolKind::Custom { name } => {
            let input = serde_json::from_str::<Value>(&arguments)
                .ok()
                .and_then(|v| v.get("input").and_then(Value::as_str).map(str::to_string))
                .unwrap_or(arguments);
            json!({
                "type": "custom_tool_call",
                "name": name,
                "input": input,
                "call_id": call_id,
            })
        }
    };
    serde_json::from_value(value).ok()
}

pub(crate) fn assistant_message(id: &str, text: &str) -> ResponseItem {
    let content = if text.is_empty() {
        json!([])
    } else {
        json!([{"type": "output_text", "text": text}])
    };
    serde_json::from_value(json!({
        "type": "message",
        "id": id,
        "role": "assistant",
        "content": content,
    }))
    .unwrap_or_else(|_| ResponseItem::Message {
        encrypted_content: None,
        status: None,
        id: None,
        role: "assistant".to_string(),
        content: Vec::new(),
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    })
}

pub(crate) fn reasoning_item(id: &str, text: &str) -> Option<ResponseItem> {
    serde_json::from_value(json!({
        "type": "reasoning",
        "id": id,
        "summary": [],
        "content": [{"type": "reasoning_text", "text": text}],
        "encrypted_content": null,
    }))
    .ok()
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
