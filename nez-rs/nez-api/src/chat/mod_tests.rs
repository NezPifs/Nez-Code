use super::*;
use crate::common::TextControls;
use crate::common::TextFormat;
use crate::common::TextFormatType;
use nez_protocol::models::ContentItem;
use nez_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use serde_json::value::RawValue;
use std::sync::Arc;

pub(crate) fn request(tools: Option<Value>) -> ResponsesApiRequest {
    let tools = tools.map(|tools| {
        let raw: Arc<RawValue> = Arc::from(
            RawValue::from_string(tools.to_string()).expect("tools serialize to raw json"),
        );
        crate::common::ResponsesApiTools::from(raw)
    });
    ResponsesApiRequest {
        model: "qwen3.5:9b".to_string(),
        stream: true,
        service_tier: None,
        input: vec![ResponseItem::Message {
            encrypted_content: None,
            status: None,
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "hello".to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }],
        tools,
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        reasoning: None,
        store: false,
        stream_options: None,
        include: Vec::new(),
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

fn shell_tool() -> Value {
    json!([{"type": "function", "name": "shell_command", "description": "Run a command",
            "strict": false, "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}])
}

#[test]
fn ollama_body_carries_sampling_context_and_thinking() {
    let mut options = ChatOptions::new(ChatDialect::Ollama);
    options.temperature = Some(0.2);
    options.top_p = Some(0.9);
    options.num_ctx = Some(16384);
    options.think = Some(false);
    let prepared = prepare(&request(Some(shell_tool())), &options).expect("prepare");
    let body = prepared.body;
    assert_eq!(body["model"], "qwen3.5:9b");
    assert_eq!(body["stream"], true);
    assert_eq!(body["think"], false);
    assert_eq!(body["options"]["num_ctx"], 16384);
    assert!((body["options"]["temperature"].as_f64().unwrap_or_default() - 0.2).abs() < 1e-6);
    assert!((body["options"]["top_p"].as_f64().unwrap_or_default() - 0.9).abs() < 1e-6);
    assert_eq!(body["tools"][0]["function"]["name"], "shell_command");
    assert_eq!(
        body["messages"],
        json!([{"role": "user", "content": "hello"}])
    );
    assert!(body.get("stream_options").is_none());
}

#[test]
fn openai_body_uses_chat_completion_fields() {
    let mut options = ChatOptions::new(ChatDialect::OpenAi);
    options.temperature = Some(0.2);
    options.max_output_tokens = Some(2048);
    let prepared = prepare(&request(Some(shell_tool())), &options).expect("prepare");
    let body = prepared.body;
    assert_eq!(body["stream_options"], json!({"include_usage": true}));
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["max_tokens"], 2048);
    assert!(body.get("options").is_none());
    assert!(body.get("think").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn reasoning_effort_none_is_opt_in_for_openai_compatible_servers() {
    let mut options = ChatOptions::new(ChatDialect::OpenAi);
    options.think = Some(false);
    options.send_reasoning_effort = true;
    let body = prepare(&request(None), &options).expect("prepare").body;
    assert_eq!(body["reasoning_effort"], "none");
}

#[test]
fn tools_are_omitted_without_native_tool_support() {
    let mut options = ChatOptions::new(ChatDialect::Ollama);
    options.native_tools = false;
    let prepared = prepare(&request(Some(shell_tool())), &options).expect("prepare");
    assert!(prepared.body.get("tools").is_none());
    // The tool table is still kept so text-protocol calls can be resolved.
    assert!(!prepared.tools.is_empty());
}

#[test]
fn output_schema_maps_to_each_dialects_structured_output_field() {
    let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
    let mut req = request(None);
    req.text = Some(TextControls {
        verbosity: None,
        format: Some(TextFormat {
            r#type: TextFormatType::JsonSchema,
            strict: true,
            schema: schema.clone(),
            name: "x".to_string(),
        }),
    });
    let ollama = prepare(&req, &ChatOptions::new(ChatDialect::Ollama))
        .expect("prepare")
        .body;
    assert_eq!(ollama["format"], schema);
    let openai = prepare(&req, &ChatOptions::new(ChatDialect::OpenAi))
        .expect("prepare")
        .body;
    assert_eq!(openai["response_format"]["type"], "json_schema");
    assert_eq!(openai["response_format"]["json_schema"]["schema"], schema);
}

#[test]
fn finish_emits_reasoning_message_calls_then_completion() {
    let tools = convert::convert_tools(Some(&shell_tool()));
    let mut acc = stream::Accumulator::new("resp".to_string());
    let lines = [
        r#"{"message":{"role":"assistant","content":"Running it.","thinking":"why"},"done":false}"#,
        r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"shell_command","arguments":{"command":"ls"}}}]},"done":true,"prompt_eval_count":3,"eval_count":4}"#,
    ];
    for line in lines {
        if let Some(chunk) = stream::parse_line(ChatDialect::Ollama, line).expect("parse") {
            acc.push(chunk, true);
        }
    }
    let events = finish(&mut acc, &tools, /*text_tools*/ false);
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| match event {
            ResponseEvent::OutputItemDone(ResponseItem::Reasoning { .. }) => "reasoning",
            ResponseEvent::OutputItemDone(ResponseItem::Message { .. }) => "message",
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. }) => "call",
            ResponseEvent::Completed { .. } => "completed",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["reasoning", "message", "call", "completed"]);
    match events.last() {
        Some(ResponseEvent::Completed {
            response_id,
            token_usage,
            ..
        }) => {
            assert_eq!(response_id, "resp");
            assert_eq!(token_usage.as_ref().map(|u| u.total_tokens), Some(7));
        }
        other => panic!("expected completion, got {other:?}"),
    }
}

fn text_options() -> ChatOptions {
    let mut options = ChatOptions::new(ChatDialect::Ollama);
    options.native_tools = false;
    options.text_tools = Some(TextToolFormat::Json);
    options.structured_output = true;
    options
}

#[test]
fn text_protocol_requests_describe_tools_instead_of_sending_them() {
    let prepared = prepare(&request(Some(shell_tool())), &text_options()).expect("prepare");
    let body = prepared.body;
    assert!(prepared.text_tools);
    assert_eq!(body.get("tools"), None);
    let system = body["messages"][0].clone();
    assert_eq!(system["role"], "system");
    let content = system["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("shell_command(command?: string)"),
        "{content}"
    );
    assert_eq!(
        body["format"]["properties"]["tool_calls"]["items"]["properties"]["tool"]["enum"],
        json!(["shell_command"])
    );
}

#[test]
fn text_protocol_without_tools_is_a_plain_request() {
    let prepared = prepare(&request(None), &text_options()).expect("prepare");
    assert!(!prepared.text_tools);
    assert_eq!(prepared.body.get("format"), None);
    assert_eq!(prepared.body["messages"][0]["role"], "user");
}

#[test]
fn finish_turns_text_tool_blocks_into_calls() {
    let tools = convert::convert_tools(Some(&shell_tool()));
    let mut acc = stream::Accumulator::new("resp".to_string());
    let line = r#"{"message":{"role":"assistant","content":"Listing.\n{\"tool\": \"shell_command\", \"input\": {\"command\": \"ls\"}}"},"done":true}"#;
    let chunk = stream::parse_line(ChatDialect::Ollama, line)
        .expect("parse")
        .expect("chunk");
    assert!(acc.push(chunk, /*stream_text*/ false).is_empty());
    let events = finish(&mut acc, &tools, /*text_tools*/ true);
    let items: Vec<&ResponseItem> = events
        .iter()
        .filter_map(|event| match event {
            ResponseEvent::OutputItemDone(item) => Some(item),
            _ => None,
        })
        .collect();
    match items.as_slice() {
        [
            ResponseItem::Message { content, .. },
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            },
        ] => {
            assert!(
                matches!(content.as_slice(), [ContentItem::OutputText { text, .. }] if text == "Listing.")
            );
            assert_eq!(name, "shell_command");
            assert_eq!(arguments, r#"{"command":"ls"}"#);
            assert_eq!(call_id, "resp_text_call_0");
        }
        other => panic!("unexpected items: {other:?}"),
    }
}

#[test]
fn unusable_text_tool_blocks_are_reported_in_the_reply() {
    let tools = convert::convert_tools(Some(&shell_tool()));
    let mut acc = stream::Accumulator::new("resp".to_string());
    let line = r#"{"message":{"role":"assistant","content":"<write_file path=\"a.txt\">x</write_file>"},"done":true}"#;
    let chunk = stream::parse_line(ChatDialect::Ollama, line)
        .expect("parse")
        .expect("chunk");
    acc.push(chunk, false);
    let events = finish(&mut acc, &tools, true);
    match &events[0] {
        ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => assert!(
            matches!(content.as_slice(), [ContentItem::OutputText { text, .. }]
                if text == "Tool call not run: File edits are not available in this session.")
        ),
        other => panic!("unexpected event: {other:?}"),
    }
    // The turn continues so the model can correct the call.
    assert!(matches!(
        events.last(),
        Some(ResponseEvent::Completed {
            end_turn: Some(false),
            ..
        })
    ));
}

fn malformed_reply() -> ResponseItem {
    serde_json::from_value(json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "Tool call not run: `ReadFile` needs a `path`."}],
    }))
    .expect("assistant message")
}

#[test]
fn malformed_tool_replies_are_retried_a_bounded_number_of_times() {
    let mut req = request(Some(shell_tool()));
    req.input.push(malformed_reply());
    let prepared = prepare(&req, &text_options()).expect("prepare");
    assert!(prepared.retry_malformed_calls);
    let messages = prepared.body["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let last = messages.last().cloned().unwrap_or_default();
    assert_eq!(last["role"], "user");
    assert!(
        last["content"]
            .as_str()
            .unwrap_or_default()
            .contains("could not be run")
    );

    req.input.push(malformed_reply());
    let prepared = prepare(&req, &text_options()).expect("prepare");
    assert!(!prepared.retry_malformed_calls);
}

#[test]
fn output_schema_does_not_replace_the_text_tool_envelope() {
    let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}},
                        "additionalProperties": false});
    let mut req = request(Some(shell_tool()));
    req.text = Some(TextControls {
        verbosity: None,
        format: Some(TextFormat {
            r#type: TextFormatType::JsonSchema,
            strict: true,
            schema,
            name: "x".to_string(),
        }),
    });
    let body = prepare(&req, &text_options()).expect("prepare").body;
    assert!(body["format"]["properties"].get("tool_calls").is_some());
    let system = body["messages"][0]["content"].as_str().unwrap_or_default();
    assert!(system.contains("final answer"), "{system}");
    assert!(
        system.contains("\"additionalProperties\":false"),
        "{system}"
    );
}

async fn pump_lines(
    dialect: ChatDialect,
    body: &'static str,
) -> Vec<Result<ResponseEvent, ApiError>> {
    use futures::StreamExt;
    let bytes = futures::stream::iter([Ok(bytes::Bytes::from_static(body.as_bytes()))]).boxed();
    let prepared = prepare(&request(None), &ChatOptions::new(dialect)).expect("prepare");
    let (tx, mut rx) = mpsc::channel(16);
    pump(bytes, tx, prepared, dialect, Duration::from_secs(5)).await;
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn a_stream_cut_off_before_the_terminal_marker_is_an_error() {
    let events = pump_lines(
        ChatDialect::Ollama,
        "{\"message\":{\"role\":\"assistant\",\"content\":\"Half a sen\"},\"done\":false}\n",
    )
    .await;
    assert!(matches!(events.last(), Some(Err(ApiError::Stream(_)))));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Ok(ResponseEvent::Completed { .. })))
    );

    let events = pump_lines(
        ChatDialect::OpenAi,
        "data: {\"choices\":[{\"delta\":{\"content\":\"Half\"}}]}\n\n",
    )
    .await;
    assert!(matches!(events.last(), Some(Err(ApiError::Stream(_)))));
}

#[tokio::test]
async fn a_finish_reason_without_done_still_completes() {
    let events = pump_lines(
        ChatDialect::OpenAi,
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":\"stop\"}]}\n\n",
    )
    .await;
    assert!(matches!(
        events.last(),
        Some(Ok(ResponseEvent::Completed { .. }))
    ));
}
