//! Tool calls written as text by models without native tool support.
//!
//! Small local models often cannot use a server-side tool template, or use it
//! unreliably. For those models the request leaves `tools` out and the system
//! prompt describes a text protocol instead (see [`protocol_instructions`]).
//! [`parse`] turns the reply back into ordinary tool calls, which then follow
//! the same approval and sandbox path as native calls.
//!
//! Accepted forms:
//!
//! - JSON: `{"tool": "exec_command", "input": {"cmd": "ls"}}`, arrays of such
//!   objects, the structured-output envelope `{"message": ..., "tool_calls": [...]}`,
//!   and the `{"name": ..., "arguments"|"parameters": ...}` shape some models
//!   copy from function-calling docs. Fenced code blocks are fine.
//! - XML shorthands: `<shell>`, `<bash>`, `<powershell description="...">`,
//!   `<read_file path="..." start="1" end="40"/>`, `<write_file path="...">`,
//!   `<edit_file path="..."><old>...</old><new>...</new></edit_file>`.
//! - DeepSeek's raw tool-call sentinel tokens.

use super::convert::ChatTools;
use super::convert::ToolKind;
use crate::chat::TextToolFormat;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

/// A tool call recovered from text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextCall {
    /// Flattened chat tool name, resolvable with [`ChatTools::kind`].
    pub name: String,
    /// JSON arguments as sent by a native call.
    pub arguments: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Parsed {
    /// Prose left after removing tool blocks.
    pub text: String,
    pub calls: Vec<TextCall>,
    /// Blocks that looked like tool calls but could not be used. Reported back
    /// to the model so it can correct itself instead of silently stalling.
    pub errors: Vec<String>,
}

const SHELL_TOOL: &str = "exec_command";
const PATCH_TOOL: &str = "apply_patch";

/// Extracts tool calls from a model reply.
pub(crate) fn parse(text: &str, tools: &ChatTools) -> Parsed {
    let cleaned = strip_deepseek_sentinels(text);
    let mut parsed = Parsed::default();
    let mut remaining = String::new();

    for segment in split_segments(&cleaned) {
        match segment {
            Segment::Text(t) => remaining.push_str(&t),
            Segment::Json(value) => {
                if let Some(message) = collect_json_calls(&value, tools, &mut parsed) {
                    remaining.push_str(&message);
                }
            }
            Segment::Xml { tag, attrs, body } => match xml_call(&tag, &attrs, &body, tools) {
                Ok(call) => parsed.calls.push(call),
                Err(err) => parsed.errors.push(err),
            },
        }
    }
    parsed.text = collapse_blank_lines(remaining.trim());
    parsed
}

/// Explains the text protocol to the model. Appended to the system prompt.
pub(crate) fn protocol_instructions(tools: &ChatTools, format: TextToolFormat) -> String {
    let mut out = String::from(
        "# Tools\n\nYou cannot call tools natively. To use a tool, write a tool block in your \
         reply and stop; the result arrives in the next message. Never claim a command ran or a \
         file changed until a result confirms it. When the task is done, answer without a tool block.\n\n",
    );
    match format {
        TextToolFormat::Json => out.push_str(
            "Write each call as one JSON object on its own:\n\
             {\"tool\": \"<tool name>\", \"input\": {<arguments>}}\n\n",
        ),
        TextToolFormat::Xml => out.push_str(
            "Use these blocks:\n\
             <shell description=\"what this changes\">command</shell>\n\
             <read_file path=\"src/main.rs\" start=\"1\" end=\"80\"/>\n\
             <write_file path=\"notes.txt\">full file contents</write_file>\n\
             <edit_file path=\"src/main.rs\"><old>exact existing lines</old><new>replacement</new></edit_file>\n\
             Other tools: {\"tool\": \"<tool name>\", \"input\": {<arguments>}}\n\n",
        ),
    }
    out.push_str("Available tools:\n");
    for definition in &tools.definitions {
        let function = &definition["function"];
        let name = function["name"].as_str().unwrap_or_default();
        let description = function["description"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default();
        let params = compact_parameters(&function["parameters"]);
        out.push_str(&format!("- {name}({params}): {description}\n"));
    }
    out
}

/// JSON schema used to constrain text-protocol replies on servers that support
/// structured output. Every reply is an envelope with optional tool calls.
pub(crate) fn structured_output_schema(tools: &ChatTools) -> Value {
    let names: Vec<Value> = tools
        .definitions
        .iter()
        .filter_map(|d| d["function"]["name"].as_str())
        .map(|n| Value::String(n.to_string()))
        .collect();
    json!({
        "type": "object",
        "properties": {
            "message": {"type": "string"},
            "tool_calls": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string", "enum": names},
                        "input": {"type": "object"}
                    },
                    "required": ["tool", "input"]
                }
            }
        },
        "required": ["message"]
    })
}

fn compact_parameters(schema: &Value) -> String {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return String::new();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    props
        .iter()
        .map(|(name, prop)| {
            let ty = prop.get("type").and_then(Value::as_str).unwrap_or("any");
            if required.contains(&name.as_str()) {
                format!("{name}: {ty}")
            } else {
                format!("{name}?: {ty}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

enum Segment {
    Text(String),
    Json(Value),
    Xml {
        tag: String,
        attrs: Vec<(String, String)>,
        body: String,
    },
}

const XML_TAGS: [&str; 7] = [
    "shell",
    "bash",
    "powershell",
    "read_file",
    "write_file",
    "edit_file",
    "tool",
];

/// Splits text into prose, JSON objects/arrays and recognized XML blocks.
fn split_segments(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut prose = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with('<')
            && let Some((block, consumed)) = parse_xml_block(rest)
        {
            flush_prose(&mut prose, &mut segments);
            segments.push(block);
            i += consumed;
            continue;
        }
        if (bytes[i] == b'{' || bytes[i] == b'[')
            && let Some((value, consumed)) = parse_json_prefix(rest)
            && looks_like_tool_json(&value)
        {
            flush_prose(&mut prose, &mut segments);
            strip_open_fence(&mut segments);
            segments.push(Segment::Json(value));
            i += consumed;
            i += skip_close_fence(&text[i..]);
            continue;
        }
        let ch = rest.chars().next().unwrap_or_default();
        prose.push(ch);
        i += ch.len_utf8();
    }
    flush_prose(&mut prose, &mut segments);
    segments
}

fn flush_prose(prose: &mut String, segments: &mut Vec<Segment>) {
    if !prose.is_empty() {
        segments.push(Segment::Text(std::mem::take(prose)));
    }
}

/// Drops a dangling ```json fence that preceded a parsed JSON block.
fn strip_open_fence(segments: &mut [Segment]) {
    if let Some(Segment::Text(t)) = segments.last_mut() {
        let trimmed = t.trim_end_matches([' ', '\t', '\n', '\r']);
        for fence in ["```json", "```JSON", "```"] {
            if let Some(stripped) = trimmed.strip_suffix(fence) {
                *t = stripped.to_string();
                return;
            }
        }
    }
}

fn skip_close_fence(rest: &str) -> usize {
    let trimmed = rest.trim_start_matches([' ', '\t', '\n', '\r']);
    if trimmed.starts_with("```") {
        rest.len() - trimmed.len() + 3
    } else {
        0
    }
}

fn parse_json_prefix(text: &str) -> Option<(Value, usize)> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => Some((value, stream.byte_offset())),
        _ => None,
    }
}

fn looks_like_tool_json(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty() && items.iter().all(looks_like_tool_json),
        Value::Object(obj) => {
            obj.contains_key("tool")
                || obj.contains_key("tool_calls")
                || (obj.contains_key("name")
                    && (obj.contains_key("arguments") || obj.contains_key("parameters")))
                || (obj.contains_key("message") && obj.len() <= 2)
        }
        _ => false,
    }
}

/// Records calls from a JSON segment. Returns any message text it carried.
fn collect_json_calls(value: &Value, tools: &ChatTools, parsed: &mut Parsed) -> Option<String> {
    match value {
        Value::Array(items) => {
            let mut message = String::new();
            for item in items {
                if let Some(m) = collect_json_calls(item, tools, parsed) {
                    message.push_str(&m);
                }
            }
            (!message.is_empty()).then_some(message)
        }
        Value::Object(obj) => {
            let mut message = obj
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(calls) = obj.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    collect_json_calls(call, tools, parsed);
                }
                return message;
            }
            let name = obj
                .get("tool")
                .or_else(|| obj.get("name"))
                .and_then(Value::as_str);
            let Some(name) = name else {
                return message.take();
            };
            let input = obj
                .get("input")
                .or_else(|| obj.get("arguments"))
                .or_else(|| obj.get("parameters"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            // Arguments are sometimes double-encoded as a string.
            let input = match input {
                Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                other => other,
            };
            match resolve_json_call(name, input, tools) {
                Ok(call) => parsed.calls.push(call),
                Err(err) => parsed.errors.push(err),
            }
            message
        }
        _ => None,
    }
}

fn resolve_json_call(name: &str, input: Value, tools: &ChatTools) -> Result<TextCall, String> {
    let Some(resolved) = resolve_tool_name(name, tools) else {
        if let Some(call) = file_alias_call(name, &input, tools) {
            return call;
        }
        // Dispatch anyway: the engine answers with an "unsupported call"
        // result, which tells the model what went wrong on its next turn.
        let arguments = match input {
            Value::Object(obj) => Value::Object(obj),
            _ => json!({}),
        };
        return Ok(TextCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        });
    };
    let arguments = match (tools.kind(&resolved), input) {
        (ToolKind::Custom { .. }, Value::String(s)) => json!({"input": s}),
        (ToolKind::Custom { .. }, Value::Object(obj)) if !obj.contains_key("input") => {
            // Accept `{"patch": "..."}` and similar single-field shapes.
            match obj.into_iter().next() {
                Some((_, Value::String(s))) => json!({"input": s}),
                _ => {
                    return Err(format!(
                        "`{resolved}` takes a single `input` string argument."
                    ));
                }
            }
        }
        (_, Value::Object(obj)) => Value::Object(obj),
        (_, other) => {
            return Err(format!(
                "Arguments for `{resolved}` must be a JSON object, got {other}."
            ));
        }
    };
    Ok(TextCall {
        name: resolved,
        arguments: arguments.to_string(),
    })
}

/// Matches a name the model wrote to an offered tool, tolerating case and the
/// aliases older Nez releases taught (`Bash`, `PowerShell`, `ReadFile`, ...).
fn resolve_tool_name(name: &str, tools: &ChatTools) -> Option<String> {
    if tools.kinds.contains_key(name) {
        return Some(name.to_string());
    }
    let lower = name.to_ascii_lowercase();
    if let Some(found) = tools.kinds.keys().find(|k| k.to_ascii_lowercase() == lower) {
        return Some(found.clone());
    }
    let alias = match lower.as_str() {
        "bash" | "shell" | "powershell" | "shell_command" | "run" | "terminal" => SHELL_TOOL,
        "patch" | "applypatch" => PATCH_TOOL,
        _ => return None,
    };
    tools.kinds.contains_key(alias).then(|| alias.to_string())
}

fn xml_call(
    tag: &str,
    attrs: &[(String, String)],
    body: &str,
    tools: &ChatTools,
) -> Result<TextCall, String> {
    let attr = |key: &str| {
        attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
    };
    match tag {
        "shell" | "bash" | "powershell" => {
            let command = body.trim();
            if command.is_empty() {
                return Err(format!("<{tag}> needs a command."));
            }
            shell_call(command, attr("description"), tools)
        }
        "read_file" => {
            let path = attr("path")
                .or_else(|| (!body.trim().is_empty()).then(|| body.trim().to_string()))
                .ok_or_else(|| "<read_file> needs a path.".to_string())?;
            let start = attr("start").or_else(|| attr("start_line"));
            let end = attr("end").or_else(|| attr("end_line"));
            let command = read_command(
                &path,
                start.and_then(|s| s.parse().ok()),
                end.and_then(|s| s.parse().ok()),
            );
            shell_call(&command, Some(format!("Read {path}")), tools)
        }
        "write_file" => {
            let path = attr("path").ok_or_else(|| "<write_file> needs a path.".to_string())?;
            patch_call(add_file_patch(&path, &strip_one_newline(body)), tools)
        }
        "edit_file" => {
            let path = attr("path").ok_or_else(|| "<edit_file> needs a path.".to_string())?;
            let old = inner_tag(body, &["old", "old_string", "search"])
                .ok_or_else(|| "<edit_file> needs an <old> block.".to_string())?;
            let new = inner_tag(body, &["new", "new_string", "replace"]).unwrap_or_default();
            patch_call(
                update_file_patch(&path, &strip_one_newline(&old), &strip_one_newline(&new)),
                tools,
            )
        }
        "tool" => {
            let name = attr("name").ok_or_else(|| "<tool> needs a name attribute.".to_string())?;
            let input: Value = serde_json::from_str(body.trim())
                .map_err(|err| format!("<tool name=\"{name}\"> body is not JSON: {err}"))?;
            resolve_json_call(&name, input, tools)
        }
        _ => Err(format!("Unsupported block <{tag}>.")),
    }
}

fn add_file_patch(path: &str, content: &str) -> String {
    let mut patch = format!("*** Begin Patch\n*** Add File: {path}\n");
    for line in content.lines() {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch.push_str("*** End Patch\n");
    patch
}

fn update_file_patch(path: &str, old: &str, new: &str) -> String {
    let mut patch = format!("*** Begin Patch\n*** Update File: {path}\n@@\n");
    for line in old.lines() {
        patch.push('-');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in new.lines() {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch.push_str("*** End Patch\n");
    patch
}

/// File tools taught by Nez 2.x (`ReadFile`, `WriteFile`, `EditFile`) map onto
/// the shell and patch tools. Returns `None` for any other name.
fn file_alias_call(
    name: &str,
    input: &Value,
    tools: &ChatTools,
) -> Option<Result<TextCall, String>> {
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| input.get(*key).and_then(Value::as_str))
            .map(str::to_string)
    };
    let number = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            let value = input.get(*key)?;
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        })
    };
    let path = field(&["path", "file_path", "filename", "file"]);
    let normalized: String = name
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_ascii_lowercase();
    let result = match normalized.as_str() {
        "readfile" | "read" | "viewfile" => path
            .ok_or_else(|| format!("`{name}` needs a `path`."))
            .and_then(|path| {
                let command = read_command(
                    &path,
                    number(&["start", "start_line", "offset"]),
                    number(&["end", "end_line"]),
                );
                shell_call(&command, Some(format!("Read {path}")), tools)
            }),
        "writefile" | "write" | "createfile" => path
            .ok_or_else(|| format!("`{name}` needs a `path`."))
            .and_then(|path| {
                let content = field(&["content", "contents", "text"]).unwrap_or_default();
                patch_call(add_file_patch(&path, &content), tools)
            }),
        "editfile" | "edit" | "strreplace" | "replaceinfile" => path
            .ok_or_else(|| format!("`{name}` needs a `path`."))
            .and_then(|path| {
                let old = field(&["old", "old_string", "old_str", "search"])
                    .ok_or_else(|| format!("`{name}` needs the `old_string` to replace."))?;
                let new = field(&["new", "new_string", "new_str", "replace"]).unwrap_or_default();
                patch_call(update_file_patch(&path, &old, &new), tools)
            }),
        _ => return None,
    };
    Some(result)
}

fn shell_call(
    command: &str,
    description: Option<String>,
    tools: &ChatTools,
) -> Result<TextCall, String> {
    if !tools.kinds.contains_key(SHELL_TOOL) {
        return Err("Shell commands are not available in this session.".to_string());
    }
    let mut args = Map::new();
    args.insert("cmd".to_string(), Value::String(command.to_string()));
    if let Some(description) = description.filter(|d| !d.trim().is_empty())
        && tool_has_param(tools, SHELL_TOOL, "description")
    {
        args.insert("description".to_string(), Value::String(description));
    }
    Ok(TextCall {
        name: SHELL_TOOL.to_string(),
        arguments: Value::Object(args).to_string(),
    })
}

fn patch_call(patch: String, tools: &ChatTools) -> Result<TextCall, String> {
    if !tools.kinds.contains_key(PATCH_TOOL) {
        return Err("File edits are not available in this session.".to_string());
    }
    Ok(TextCall {
        name: PATCH_TOOL.to_string(),
        arguments: json!({"input": patch}).to_string(),
    })
}

fn tool_has_param(tools: &ChatTools, name: &str, param: &str) -> bool {
    tools.definitions.iter().any(|d| {
        d["function"]["name"].as_str() == Some(name)
            && d["function"]["parameters"]["properties"]
                .get(param)
                .is_some()
    })
}

/// Builds a read-only command that prints a file, optionally a line range.
fn read_command(path: &str, start: Option<u64>, end: Option<u64>) -> String {
    if cfg!(windows) {
        let quoted = format!("'{}'", path.replace('\'', "''"));
        match (start, end) {
            (None, None) => format!("Get-Content -LiteralPath {quoted}"),
            (start, end) => {
                let skip = start.unwrap_or(1).saturating_sub(1);
                let take = end.map_or(String::new(), |e| {
                    format!(" -First {}", e.saturating_sub(skip))
                });
                format!("Get-Content -LiteralPath {quoted} | Select-Object -Skip {skip}{take}")
            }
        }
    } else {
        let quoted = format!("'{}'", path.replace('\'', "'\\''"));
        match (start, end) {
            (None, None) => format!("cat -- {quoted}"),
            (start, end) => {
                let start = start.unwrap_or(1).max(1);
                let end = end.map_or("$".to_string(), |e| e.to_string());
                format!("sed -n '{start},{end}p' -- {quoted}")
            }
        }
    }
}

fn strip_one_newline(s: &str) -> String {
    let s = s
        .strip_prefix("\r\n")
        .or_else(|| s.strip_prefix('\n'))
        .unwrap_or(s);
    let s = s
        .strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s);
    s.to_string()
}

fn inner_tag(body: &str, names: &[&str]) -> Option<String> {
    for name in names {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        if let Some(start) = body.find(&open) {
            let from = start + open.len();
            if let Some(len) = body[from..].find(&close) {
                return Some(body[from..from + len].to_string());
            }
        }
    }
    None
}

/// Parses `<tag attr="v">body</tag>` or `<tag attr="v"/>` at the start of `text`.
/// Returns the block as a [`Segment::Xml`] and the number of bytes consumed.
fn parse_xml_block(text: &str) -> Option<(Segment, usize)> {
    let after_lt = &text[1..];
    let tag_len = after_lt
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after_lt.len());
    let tag = after_lt[..tag_len].to_ascii_lowercase();
    if !XML_TAGS.contains(&tag.as_str()) {
        return None;
    }
    let head_end = text.find('>')?;
    let head = &text[1 + tag_len..head_end];
    let self_closing = head.trim_end().ends_with('/');
    let attrs = parse_attrs(head.trim_end_matches('/'));
    if self_closing {
        let block = Segment::Xml {
            tag,
            attrs,
            body: String::new(),
        };
        return Some((block, head_end + 1));
    }
    let close = format!("</{tag}>");
    let body_start = head_end + 1;
    let lower_rest = text[body_start..].to_ascii_lowercase();
    let body_len = lower_rest.find(&close)?;
    let body = text[body_start..body_start + body_len].to_string();
    let consumed = body_start + body_len + close.len();
    Some((Segment::Xml { tag, attrs, body }, consumed))
}

fn parse_attrs(head: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = head.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(end) = after[1..].find(quote) else {
            break;
        };
        attrs.push((key, after[1..1 + end].to_string()));
        rest = after[end + 2..].trim_start();
    }
    attrs
}

/// DeepSeek Coder V2 can emit its raw tool template instead of plain JSON:
/// `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>name\n```json\n{..}\n```<｜tool▁call▁end｜>`.
/// Rewrites those markers into the JSON protocol.
fn strip_deepseek_sentinels(text: &str) -> String {
    if !text.contains("tool▁") && !text.contains("tool_calls_begin") {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<｜tool▁call▁begin｜>") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "<｜tool▁call▁begin｜>".len()..];
        let end = after.find("<｜tool▁call▁end｜>").unwrap_or(after.len());
        let call = &after[..end];
        if let Some((_, name_and_args)) = call.split_once("<｜tool▁sep｜>") {
            let (name, args) = name_and_args
                .split_once('\n')
                .unwrap_or((name_and_args, "{}"));
            let args = args
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            let input: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
            out.push_str(&json!({"tool": name.trim(), "input": input}).to_string());
            out.push('\n');
        }
        rest = after
            .get(end + "<｜tool▁call▁end｜>".len()..)
            .unwrap_or_default();
    }
    out.push_str(rest);
    for marker in [
        "<｜tool▁calls▁begin｜>",
        "<｜tool▁calls▁end｜>",
        "<｜end▁of▁sentence｜>",
    ] {
        out = out.replace(marker, "");
    }
    out
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
#[path = "text_tools_tests.rs"]
mod tests;
