//! History checks that keep local models on task.
//!
//! These run on the Responses-shaped history before each chat request:
//!
//! - [`collapse_duplicate_results`] shortens repeated identical tool output,
//!   which small context windows cannot afford.
//! - [`goal_reminder`] restates the user's request after a long run of tool
//!   calls, so the model does not lose the goal behind its own output.
//! - [`repeated_failure`] stops the turn when the same call has failed three
//!   times in a row, instead of letting the model retry forever.

use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

/// Tool calls since the last user message before the goal is restated.
pub(crate) const GOAL_REMINDER_AFTER: usize = 6;
/// Identical failing calls in a row that end the turn.
pub(crate) const REPEATED_FAILURE_LIMIT: usize = 3;
/// Shorter outputs are cheap enough to keep.
const MIN_COLLAPSE_LEN: usize = 200;
const COLLAPSED_OUTPUT: &str = "(Same output as a later identical call.)";

fn item_type(item: &Value) -> &str {
    item.get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
}

fn is_call(item: &Value) -> bool {
    matches!(
        item_type(item),
        "function_call" | "custom_tool_call" | "local_shell_call"
    )
}

fn is_output(item: &Value) -> bool {
    matches!(
        item_type(item),
        "function_call_output" | "custom_tool_call_output"
    )
}

/// The call's name and arguments, used to recognise repeats.
fn call_signature(item: &Value) -> String {
    let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
    let args = item
        .get("arguments")
        .or_else(|| item.get("input"))
        .or_else(|| item.get("action"))
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    format!("{name}\u{0}{args}")
}

fn output_text(item: &Value) -> Option<&str> {
    item.get("output").and_then(Value::as_str)
}

/// Text of a user message typed by the user, skipping the context messages
/// Nez-Code injects (environment, instructions), which start with a tag.
fn user_text(item: &Value) -> Option<String> {
    if item_type(item) != "message" || item.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let text = match item.get("content")? {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let trimmed = text.trim();
    (!trimmed.is_empty() && !trimmed.starts_with('<')).then(|| trimmed.to_string())
}

fn last_user_index(input: &[Value]) -> Option<usize> {
    input.iter().rposition(|item| user_text(item).is_some())
}

pub(crate) fn collapse_duplicate_results(input: &mut [Value]) {
    let signatures: HashMap<String, String> = input
        .iter()
        .filter(|item| is_call(item))
        .filter_map(|item| {
            let id = item.get("call_id").and_then(Value::as_str)?;
            Some((id.to_string(), call_signature(item)))
        })
        .collect();
    let mut seen = HashSet::new();
    for item in input.iter_mut().rev() {
        if !is_output(item) {
            continue;
        }
        let Some(signature) = item
            .get("call_id")
            .and_then(Value::as_str)
            .and_then(|id| signatures.get(id))
        else {
            continue;
        };
        // Only plain-text outputs; content arrays may carry images.
        let Some(text) = output_text(item) else {
            continue;
        };
        if text.len() < MIN_COLLAPSE_LEN {
            continue;
        }
        if !seen.insert((signature.clone(), text.to_string())) {
            item["output"] = Value::String(COLLAPSED_OUTPUT.to_string());
        }
    }
}

pub(crate) fn goal_reminder(input: &[Value]) -> Option<String> {
    let index = last_user_index(input)?;
    let calls = input[index..].iter().filter(|item| is_call(item)).count();
    if calls < GOAL_REMINDER_AFTER {
        return None;
    }
    let goal = user_text(&input[index])?;
    let goal = truncate(&goal, 600);
    Some(format!(
        "Reminder of the request you are working on:\n\n{goal}\n\nKeep going until it is done. \
         If the latest tool results already complete it, stop calling tools and give the final answer."
    ))
}

pub(crate) fn repeated_failure(input: &[Value]) -> Option<String> {
    let start = last_user_index(input).unwrap_or(0);
    let recent = &input[start..];
    let outputs: HashMap<&str, &str> = recent
        .iter()
        .filter(|item| is_output(item))
        .filter_map(|item| Some((item.get("call_id")?.as_str()?, output_text(item)?)))
        .collect();
    let calls: Vec<&Value> = recent.iter().filter(|item| is_call(item)).collect();
    if calls.len() < REPEATED_FAILURE_LIMIT {
        return None;
    }
    let last = &calls[calls.len() - REPEATED_FAILURE_LIMIT..];
    let signature = call_signature(last[0]);
    let mut last_output = "";
    for call in last {
        if call_signature(call) != signature {
            return None;
        }
        let output = call
            .get("call_id")
            .and_then(Value::as_str)
            .and_then(|id| outputs.get(id))?;
        if !looks_failed(output) {
            return None;
        }
        last_output = output;
    }
    let name = last[0]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    Some(format!(
        "I stopped because `{name}` failed {REPEATED_FAILURE_LIMIT} times in a row with the same \
         arguments. Last result:\n\n{}\n\nTell me how to proceed, or ask again with more detail.",
        truncate(last_output.trim(), 1200)
    ))
}

fn looks_failed(output: &str) -> bool {
    if let Some(code) = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Process exited with code "))
    {
        return code.trim() != "0";
    }
    let lower = output.trim_start().to_ascii_lowercase();
    lower.starts_with("error")
        || lower.starts_with("failed")
        || lower.starts_with("unsupported call")
        || lower.contains("verification failed")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    format!("{cut}…")
}

#[cfg(test)]
#[path = "guards_tests.rs"]
mod tests;
