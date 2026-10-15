//! Translation from Responses-shaped requests to chat-style message lists.
//!
//! The rest of Nez speaks the Responses item model. Local servers and most hosted
//! OpenAI-compatible providers only implement chat completions, so requests are
//! rewritten here and model output is mapped back by [`super::stream`].
//!
//! Conversion works on the serialized JSON form of the request. That keeps this
//! module independent of the many internal-only fields on `ResponseItem` and
//! lets unknown item kinds fall through without breaking older history.

use super::ChatDialect;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

/// Separator used to flatten namespaced tools into a single function name.
pub(crate) const NAMESPACE_SEPARATOR: &str = "__";

/// Text substituted for images when the request must not carry pixels.
pub const IMAGE_OMITTED_TEXT: &str = "[image omitted: the selected model does not accept images]";

/// How a flattened chat tool maps back onto a Nez tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolKind {
    /// A JSON function tool. `namespace` is set for namespaced tools.
    Function {
        name: String,
        namespace: Option<String>,
    },
    /// A freeform tool exposed to chat models as a function with one `input` string.
    Custom { name: String },
}

/// Tool definitions prepared for a chat request.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChatTools {
    /// Chat-completions `tools` entries.
    pub definitions: Vec<Value>,
    /// Flattened name -> original tool.
    pub kinds: HashMap<String, ToolKind>,
}

impl ChatTools {
    pub(crate) fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    pub(crate) fn kind(&self, name: &str) -> ToolKind {
        self.kinds
            .get(name)
            .cloned()
            .unwrap_or_else(|| ToolKind::Function {
                name: name.to_string(),
                namespace: None,
            })
    }
}

/// Builds chat tool definitions from the Responses `tools` array.
///
/// Hosted-only tools (web search, tool search, image generation) have no chat
/// equivalent and are skipped.
pub(crate) fn convert_tools(tools: Option<&Value>) -> ChatTools {
    let mut out = ChatTools::default();
    let Some(Value::Array(tools)) = tools else {
        return out;
    };
    for tool in tools {
        match tool.get("type").and_then(Value::as_str) {
            Some("function") => push_function(&mut out, tool, None),
            Some("custom") => push_custom(&mut out, tool),
            Some("namespace") => {
                let Some(namespace) = tool.get("name").and_then(Value::as_str) else {
                    continue;
                };
                for inner in tool
                    .get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match inner.get("type").and_then(Value::as_str) {
                        Some("function") => push_function(&mut out, inner, Some(namespace)),
                        Some("custom") => push_custom(&mut out, inner),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn push_function(out: &mut ChatTools, tool: &Value, namespace: Option<&str>) {
    let Some(name) = tool.get("name").and_then(Value::as_str) else {
        return;
    };
    let flat = match namespace {
        Some(ns) => format!("{ns}{NAMESPACE_SEPARATOR}{name}"),
        None => name.to_string(),
    };
    let parameters = tool
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    out.definitions.push(json!({
        "type": "function",
        "function": {
            "name": flat,
            "description": tool.get("description").cloned().unwrap_or(Value::String(String::new())),
            "parameters": parameters,
        }
    }));
    out.kinds.insert(
        flat,
        ToolKind::Function {
            name: name.to_string(),
            namespace: namespace.map(str::to_string),
        },
    );
}

fn push_custom(out: &mut ChatTools, tool: &Value) {
    let Some(name) = tool.get("name").and_then(Value::as_str) else {
        return;
    };
    let mut description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(definition) = tool
        .get("format")
        .and_then(|format| format.get("definition"))
        .and_then(Value::as_str)
    {
        description.push_str("\n\nThe `input` string must follow this grammar:\n");
        description.push_str(definition);
    }
    out.definitions.push(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": {"input": {"type": "string"}},
                "required": ["input"],
            },
        }
    }));
    out.kinds.insert(
        name.to_string(),
        ToolKind::Custom {
            name: name.to_string(),
        },
    );
}

/// Options that change how history is rendered.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConvertOptions {
    pub dialect: ChatDialect,
    /// When false, images are replaced with [`IMAGE_OMITTED_TEXT`].
    pub allow_images: bool,
    /// When false, tool calls and results are rendered as plain text so models
    /// without native tool support still see what happened.
    pub native_tools: bool,
}

/// A message under construction, kept dialect-neutral until [`finish`].
#[derive(Debug, Default)]
struct Pending {
    role: String,
    text: String,
    images: Vec<String>,
    tool_calls: Vec<Value>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
}

/// Converts Responses input items into chat messages.
///
/// `instructions` is the base system prompt. Leading system/developer messages
/// are merged into it because several local chat templates only honour the
/// first system message.
pub(crate) fn convert_input(
    instructions: &str,
    input: &[Value],
    options: ConvertOptions,
) -> Vec<Value> {
    let mut system = instructions.trim().to_string();
    let mut messages: Vec<Pending> = Vec::new();
    // Tool calls whose results arrive later; used to name Ollama tool messages.
    let mut call_names: HashMap<String, String> = HashMap::new();
    // Images returned by tools are delivered in a user message after the
    // tool results, which keeps the required call/result ordering intact.
    let mut deferred_images: Vec<String> = Vec::new();

    for item in input {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message");
        if kind != "function_call_output" && kind != "custom_tool_call_output" {
            flush_deferred_images(&mut messages, &mut deferred_images);
        }
        match kind {
            "message" => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                let (text, images) = content_parts(item.get("content"));
                match role {
                    "system" | "developer" => {
                        if messages.is_empty() {
                            append_paragraph(&mut system, &text);
                        } else if !text.trim().is_empty() {
                            messages.push(Pending {
                                role: "system".to_string(),
                                text,
                                ..Default::default()
                            });
                        }
                    }
                    "assistant" => {
                        if text.is_empty() {
                            continue;
                        }
                        match messages.last_mut() {
                            Some(last)
                                if last.role == "assistant" && last.tool_calls.is_empty() =>
                            {
                                append_paragraph(&mut last.text, &text);
                            }
                            _ => messages.push(Pending {
                                role: "assistant".to_string(),
                                text,
                                ..Default::default()
                            }),
                        }
                    }
                    _ => messages.push(Pending {
                        role: "user".to_string(),
                        text,
                        images,
                        ..Default::default()
                    }),
                }
            }
            "function_call" | "custom_tool_call" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                let flat = match item.get("namespace").and_then(Value::as_str) {
                    Some(ns) if !ns.is_empty() => format!("{ns}{NAMESPACE_SEPARATOR}{name}"),
                    _ => name.to_string(),
                };
                let arguments = if kind == "custom_tool_call" {
                    let input = item
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    json!({ "input": input }).to_string()
                } else {
                    item.get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}")
                        .to_string()
                };
                call_names.insert(call_id.clone(), flat.clone());
                if options.native_tools {
                    let call = json!({
                        "id": call_id,
                        "type": "function",
                        "function": {"name": flat, "arguments": arguments},
                    });
                    match messages.last_mut() {
                        Some(last) if last.role == "assistant" => last.tool_calls.push(call),
                        _ => messages.push(Pending {
                            role: "assistant".to_string(),
                            tool_calls: vec![call],
                            ..Default::default()
                        }),
                    }
                } else {
                    let rendered =
                        format!("{{\"tool\":{},\"input\":{arguments}}}", Value::String(flat));
                    match messages.last_mut() {
                        Some(last) if last.role == "assistant" => {
                            append_paragraph(&mut last.text, &rendered)
                        }
                        _ => messages.push(Pending {
                            role: "assistant".to_string(),
                            text: rendered,
                            ..Default::default()
                        }),
                    }
                }
            }
            "function_call_output" | "custom_tool_call_output" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let (text, images) = tool_output_parts(item.get("output"));
                deferred_images.extend(images);
                let tool_name = call_names.get(&call_id).cloned();
                if options.native_tools {
                    messages.push(Pending {
                        role: "tool".to_string(),
                        text,
                        tool_call_id: Some(call_id),
                        tool_name,
                        ..Default::default()
                    });
                } else {
                    let label = tool_name.unwrap_or_else(|| "tool".to_string());
                    messages.push(Pending {
                        role: "user".to_string(),
                        text: format!("Result of {label}:\n{text}"),
                        ..Default::default()
                    });
                }
            }
            // Reasoning is never replayed; hosted-only items have no chat form.
            _ => {}
        }
    }
    flush_deferred_images(&mut messages, &mut deferred_images);

    let mut out = Vec::with_capacity(messages.len() + 1);
    if !system.is_empty() {
        out.push(json!({"role": "system", "content": system}));
    }
    out.extend(
        merge_text_only_users(messages)
            .into_iter()
            .map(|m| finish(m, options)),
    );
    out
}

fn flush_deferred_images(messages: &mut Vec<Pending>, deferred: &mut Vec<String>) {
    if deferred.is_empty() {
        return;
    }
    messages.push(Pending {
        role: "user".to_string(),
        text: "Images returned by the previous tool call.".to_string(),
        images: std::mem::take(deferred),
        ..Default::default()
    });
}

/// Consecutive plain user messages are merged: some chat templates reject
/// two user turns in a row.
fn merge_text_only_users(messages: Vec<Pending>) -> Vec<Pending> {
    let mut out: Vec<Pending> = Vec::with_capacity(messages.len());
    for message in messages {
        if message.role == "user"
            && let Some(last) = out.last_mut()
            && last.role == "user"
        {
            append_paragraph(&mut last.text, &message.text);
            last.images.extend(message.images);
            continue;
        }
        out.push(message);
    }
    out
}

fn finish(message: Pending, options: ConvertOptions) -> Value {
    let mut obj = Map::new();
    obj.insert("role".to_string(), Value::String(message.role.clone()));
    let mut text = message.text;
    let images = if options.allow_images {
        message.images
    } else {
        if !message.images.is_empty() {
            append_paragraph(&mut text, IMAGE_OMITTED_TEXT);
        }
        Vec::new()
    };

    match options.dialect {
        ChatDialect::OpenAi => {
            if images.is_empty() {
                obj.insert("content".to_string(), Value::String(text));
            } else {
                let mut parts = Vec::new();
                if !text.is_empty() {
                    parts.push(json!({"type": "text", "text": text}));
                }
                for url in images {
                    parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
                obj.insert("content".to_string(), Value::Array(parts));
            }
            if !message.tool_calls.is_empty() {
                obj.insert("tool_calls".to_string(), Value::Array(message.tool_calls));
            }
            if let Some(id) = message.tool_call_id {
                obj.insert("tool_call_id".to_string(), Value::String(id));
            }
        }
        ChatDialect::Ollama => {
            obj.insert("content".to_string(), Value::String(text));
            let encoded: Vec<Value> = images
                .iter()
                .filter_map(|url| ollama_image(url))
                .map(Value::String)
                .collect();
            if !encoded.is_empty() {
                obj.insert("images".to_string(), Value::Array(encoded));
            }
            if !message.tool_calls.is_empty() {
                let calls = message
                    .tool_calls
                    .into_iter()
                    .map(|call| {
                        let function = call.get("function").cloned().unwrap_or_default();
                        let arguments = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                            .unwrap_or_else(|| json!({}));
                        json!({
                            "function": {
                                "name": function.get("name").cloned().unwrap_or_default(),
                                "arguments": arguments,
                            }
                        })
                    })
                    .collect();
                obj.insert("tool_calls".to_string(), Value::Array(calls));
            }
            if let Some(name) = message.tool_name {
                obj.insert("tool_name".to_string(), Value::String(name));
            }
        }
    }
    Value::Object(obj)
}

/// Ollama's native API takes raw base64 without the data URL prefix and has no
/// support for remote image URLs.
fn ollama_image(url: &str) -> Option<String> {
    let rest = url.strip_prefix("data:")?;
    let (_, data) = rest.split_once(";base64,")?;
    Some(data.to_string())
}

fn content_parts(content: Option<&Value>) -> (String, Vec<String>) {
    let mut text = String::new();
    let mut images = Vec::new();
    match content {
        Some(Value::String(s)) => text.push_str(s),
        Some(Value::Array(parts)) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        if let Some(s) = part.get("text").and_then(Value::as_str) {
                            append_paragraph(&mut text, s);
                        }
                    }
                    Some("input_image") => {
                        if let Some(url) = image_url(part) {
                            images.push(url);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    (text, images)
}

fn image_url(part: &Value) -> Option<String> {
    match part.get("image_url") {
        Some(Value::String(url)) => Some(url.clone()),
        Some(Value::Object(obj)) => obj.get("url").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn tool_output_parts(output: Option<&Value>) -> (String, Vec<String>) {
    match output {
        Some(Value::String(s)) => (s.clone(), Vec::new()),
        Some(Value::Array(_)) => content_parts(output),
        Some(Value::Object(obj)) => {
            if let Some(content) = obj.get("content") {
                return tool_output_parts(Some(content));
            }
            if let Some(items) = obj.get("content_items") {
                return tool_output_parts(Some(items));
            }
            (Value::Object(obj.clone()).to_string(), Vec::new())
        }
        Some(other) => (other.to_string(), Vec::new()),
        None => (String::new(), Vec::new()),
    }
}

fn append_paragraph(buf: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push_str("\n\n");
    }
    buf.push_str(text);
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
