use super::*;
use pretty_assertions::assert_eq;

fn tools() -> ChatTools {
    super::super::convert::convert_tools(Some(&json!([
        {"type": "function", "name": "shell_command", "description": "", "strict": false,
         "parameters": {"type": "object", "properties": {}}},
        {"type": "custom", "name": "apply_patch", "description": ""},
    ])))
}

#[test]
fn line_splitter_handles_split_utf8_and_partial_lines() {
    let mut splitter = LineSplitter::default();
    let text = "data: {\"a\":\"é\"}\n\ndata: [DONE]\n";
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    for chunk in bytes.chunks(3) {
        lines.extend(splitter.push(chunk));
    }
    assert_eq!(lines, vec!["data: {\"a\":\"é\"}", "", "data: [DONE]"]);
    assert_eq!(splitter.finish(), None);
}

#[test]
fn openai_stream_assembles_fragmented_tool_calls() {
    let lines = [
        r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","content":"Let me look."}}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"shell_","arguments":""}}]}}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"command","arguments":"{\"command\":"}}]}}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}"#,
        r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":5,"total_tokens":17}}"#,
        "data: [DONE]",
    ];
    let mut acc = Accumulator::new("r".to_string());
    let mut streamed = Vec::new();
    for line in lines {
        if let Some(chunk) = parse_line(ChatDialect::OpenAi, line).expect("parse") {
            streamed.extend(acc.push(chunk, true));
        }
    }
    assert!(acc.done);
    assert_eq!(acc.text, "Let me look.");
    assert_eq!(acc.finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(acc.usage.as_ref().map(|u| u.total_tokens), Some(17));
    assert!(matches!(streamed[0], ResponseEvent::OutputItemAdded(_)));
    assert!(matches!(&streamed[1], ResponseEvent::OutputTextDelta(d) if d == "Let me look."));

    let calls = acc.take_tool_calls(&tools());
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        ResponseItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } => {
            assert_eq!(name, "shell_command");
            assert_eq!(arguments, "{\"command\":\"ls\"}");
            assert_eq!(call_id, "call_1");
        }
        other => panic!("unexpected item {other:?}"),
    }
}

#[test]
fn ollama_stream_reads_thinking_tool_calls_and_counts() {
    let lines = [
        r#"{"message":{"role":"assistant","content":"","thinking":"plan"},"done":false}"#,
        r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"shell_command","arguments":{"command":"ls"}}},{"function":{"name":"apply_patch","arguments":{"input":"*** Begin Patch"}}}]},"done":false}"#,
        r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":40,"eval_count":9}"#,
    ];
    let mut acc = Accumulator::new("r".to_string());
    for line in lines {
        if let Some(chunk) = parse_line(ChatDialect::Ollama, line).expect("parse") {
            acc.push(chunk, true);
        }
    }
    assert!(acc.done);
    assert_eq!(acc.reasoning, "plan");
    assert_eq!(acc.usage.as_ref().map(|u| u.input_tokens), Some(40));
    let calls = acc.take_tool_calls(&tools());
    assert_eq!(calls.len(), 2);
    assert!(
        matches!(&calls[0], ResponseItem::FunctionCall { arguments, call_id, .. }
            if arguments == "{\"command\":\"ls\"}" && call_id == "r_call_0")
    );
    assert!(
        matches!(&calls[1], ResponseItem::CustomToolCall { name, input, .. }
            if name == "apply_patch" && input == "*** Begin Patch")
    );
}

#[test]
fn stream_errors_surface_as_api_errors() {
    let err = parse_line(ChatDialect::Ollama, r#"{"error":"model 'x' not found"}"#)
        .expect_err("error chunk");
    assert!(err.to_string().contains("model 'x' not found"));
    let err = parse_line(
        ChatDialect::OpenAi,
        r#"data: {"error":{"message":"Insufficient Balance"}}"#,
    )
    .expect_err("error chunk");
    assert!(err.to_string().contains("Insufficient Balance"));
}

#[test]
fn keep_alive_lines_are_ignored() {
    assert_eq!(
        parse_line(ChatDialect::OpenAi, ": keep-alive").ok(),
        Some(None)
    );
    assert_eq!(
        parse_line(ChatDialect::OpenAi, "event: ping").ok(),
        Some(None)
    );
    assert_eq!(parse_line(ChatDialect::Ollama, "   ").ok(), Some(None));
}

#[test]
fn deepseek_cache_hits_count_as_cached_input() {
    let chunk = parse_body(
        ChatDialect::OpenAi,
        r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":2,"total_tokens":102,"prompt_cache_hit_tokens":80}}"#,
    )
    .expect("body");
    assert_eq!(chunk.content.as_deref(), Some("ok"));
    assert_eq!(chunk.usage.map(|u| u.cached_input_tokens), Some(80));
}
