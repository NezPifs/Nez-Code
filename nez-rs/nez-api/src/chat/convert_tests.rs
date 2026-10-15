use super::*;
use pretty_assertions::assert_eq;

fn opts(dialect: ChatDialect) -> ConvertOptions {
    ConvertOptions {
        dialect,
        allow_images: true,
        native_tools: true,
    }
}

fn user(text: &str) -> Value {
    json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]})
}

#[test]
fn leading_developer_messages_merge_into_one_system_message() {
    let input = vec![
        json!({"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Be brief."}]}),
        json!({"type": "message", "role": "system", "content": [{"type": "input_text", "text": "Use tools."}]}),
        user("hi"),
    ];
    let messages = convert_input("Base.", &input, opts(ChatDialect::OpenAi));
    assert_eq!(
        messages,
        vec![
            json!({"role": "system", "content": "Base.\n\nBe brief.\n\nUse tools."}),
            json!({"role": "user", "content": "hi"}),
        ]
    );
}

#[test]
fn tool_calls_and_results_use_native_shapes_for_openai() {
    let input = vec![
        user("list files"),
        json!({"type": "function_call", "name": "shell_command", "arguments": "{\"command\":\"ls\"}", "call_id": "c1"}),
        json!({"type": "function_call", "name": "shell_command", "arguments": "{\"command\":\"pwd\"}", "call_id": "c2"}),
        json!({"type": "function_call_output", "call_id": "c1", "output": "a.txt"}),
        json!({"type": "function_call_output", "call_id": "c2", "output": "/tmp"}),
    ];
    let messages = convert_input("", &input, opts(ChatDialect::OpenAi));
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"].as_array().map(Vec::len), Some(2));
    assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
    assert_eq!(
        messages[2],
        json!({"role": "tool", "content": "a.txt", "tool_call_id": "c1"})
    );
    assert_eq!(messages[3]["tool_call_id"], "c2");
}

#[test]
fn ollama_tool_calls_carry_object_arguments_and_tool_names() {
    let input = vec![
        user("go"),
        json!({"type": "function_call", "name": "shell_command", "arguments": "{\"command\":\"ls\"}", "call_id": "c1"}),
        json!({"type": "function_call_output", "call_id": "c1", "output": "ok"}),
    ];
    let messages = convert_input("", &input, opts(ChatDialect::Ollama));
    assert_eq!(
        messages[1]["tool_calls"][0],
        json!({"function": {"name": "shell_command", "arguments": {"command": "ls"}}})
    );
    assert_eq!(
        messages[2],
        json!({"role": "tool", "content": "ok", "tool_name": "shell_command"})
    );
}

#[test]
fn custom_tool_calls_round_trip_through_an_input_argument() {
    let input = vec![
        user("patch"),
        json!({"type": "custom_tool_call", "name": "apply_patch", "input": "*** Begin Patch", "call_id": "p1"}),
        json!({"type": "custom_tool_call_output", "call_id": "p1", "output": "Done"}),
    ];
    let messages = convert_input("", &input, opts(ChatDialect::OpenAi));
    let arguments = messages[1]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(
        serde_json::from_str::<Value>(arguments).ok(),
        Some(json!({"input": "*** Begin Patch"}))
    );
}

#[test]
fn images_in_user_messages_become_image_parts() {
    let input = vec![json!({
        "type": "message",
        "role": "user",
        "content": [
            {"type": "input_text", "text": "what is this"},
            {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
        ]
    })];
    let openai = convert_input("", &input, opts(ChatDialect::OpenAi));
    assert_eq!(
        openai[0]["content"],
        json!([
            {"type": "text", "text": "what is this"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ])
    );
    let ollama = convert_input("", &input, opts(ChatDialect::Ollama));
    assert_eq!(ollama[0]["content"], "what is this");
    assert_eq!(ollama[0]["images"], json!(["AAAA"]));
}

#[test]
fn images_from_tool_output_follow_the_tool_results() {
    let input = vec![
        user("look"),
        json!({"type": "function_call", "name": "view_image", "arguments": "{}", "call_id": "v1"}),
        json!({"type": "function_call_output", "call_id": "v1", "output": [
            {"type": "input_text", "text": "screenshot"},
            {"type": "input_image", "image_url": "data:image/jpeg;base64,BBBB"}
        ]}),
    ];
    let messages = convert_input("", &input, opts(ChatDialect::OpenAi));
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["content"], "screenshot");
    assert_eq!(messages[3]["role"], "user");
    assert_eq!(
        messages[3]["content"][1]["image_url"]["url"],
        "data:image/jpeg;base64,BBBB"
    );
}

#[test]
fn text_only_models_get_a_note_instead_of_pixels() {
    let input = vec![json!({
        "type": "message",
        "role": "user",
        "content": [
            {"type": "input_text", "text": "describe"},
            {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}
        ]
    })];
    let mut options = opts(ChatDialect::Ollama);
    options.allow_images = false;
    let messages = convert_input("", &input, options);
    assert_eq!(
        messages[0],
        json!({"role": "user", "content": format!("describe\n\n{IMAGE_OMITTED_TEXT}")})
    );
}

#[test]
fn text_tool_mode_renders_calls_and_results_as_text() {
    let input = vec![
        user("go"),
        json!({"type": "function_call", "name": "shell_command", "arguments": "{\"command\":\"ls\"}", "call_id": "c1"}),
        json!({"type": "function_call_output", "call_id": "c1", "output": "a.txt"}),
    ];
    let mut options = opts(ChatDialect::Ollama);
    options.native_tools = false;
    let messages = convert_input("", &input, options);
    assert_eq!(
        messages,
        vec![
            json!({"role": "user", "content": "go"}),
            json!({"role": "assistant", "content": "{\"tool\":\"shell_command\",\"input\":{\"command\":\"ls\"}}"}),
            json!({"role": "user", "content": "Result of shell_command:\na.txt"}),
        ]
    );
}

#[test]
fn reasoning_and_hosted_items_are_dropped() {
    let input = vec![
        user("q"),
        json!({"type": "reasoning", "summary": [], "content": [{"type": "reasoning_text", "text": "hmm"}]}),
        json!({"type": "web_search_call", "status": "completed"}),
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "a"}]}),
    ];
    let messages = convert_input("", &input, opts(ChatDialect::OpenAi));
    assert_eq!(
        messages,
        vec![
            json!({"role": "user", "content": "q"}),
            json!({"role": "assistant", "content": "a"}),
        ]
    );
}

#[test]
fn tools_are_flattened_and_remembered() {
    let tools = json!([
        {"type": "function", "name": "shell_command", "description": "Run", "strict": false,
         "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}},
        {"type": "custom", "name": "apply_patch", "description": "Patch",
         "format": {"type": "grammar", "syntax": "lark", "definition": "start: x"}},
        {"type": "namespace", "name": "mcp_fs", "description": "", "tools": [
            {"type": "function", "name": "read", "description": "Read", "strict": false,
             "parameters": {"type": "object", "properties": {}}}
        ]},
        {"type": "web_search"}
    ]);
    let converted = convert_tools(Some(&tools));
    let names: Vec<&str> = converted
        .definitions
        .iter()
        .filter_map(|d| d["function"]["name"].as_str())
        .collect();
    assert_eq!(names, vec!["shell_command", "apply_patch", "mcp_fs__read"]);
    assert_eq!(
        converted.kind("apply_patch"),
        ToolKind::Custom {
            name: "apply_patch".to_string()
        }
    );
    assert_eq!(
        converted.kind("mcp_fs__read"),
        ToolKind::Function {
            name: "read".to_string(),
            namespace: Some("mcp_fs".to_string())
        }
    );
    assert!(
        converted.definitions[1]["function"]["description"]
            .as_str()
            .is_some_and(|d| d.contains("start: x"))
    );
}
