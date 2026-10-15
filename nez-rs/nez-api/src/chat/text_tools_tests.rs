use super::*;
use crate::chat::convert::convert_tools;
use pretty_assertions::assert_eq;

fn tools() -> ChatTools {
    convert_tools(Some(&json!([
        {"type": "function", "name": "exec_command", "description": "Runs a command.\nMore detail.",
         "strict": false, "parameters": {"type": "object",
            "properties": {"cmd": {"type": "string"}, "workdir": {"type": "string"}, "description": {"type": "string"}},
            "required": ["cmd"]}},
        {"type": "custom", "name": "apply_patch", "description": "Edit files."},
        {"type": "function", "name": "update_plan", "description": "Plan.", "strict": false,
         "parameters": {"type": "object", "properties": {"plan": {"type": "array"}}}}
    ])))
}

fn args(call: &TextCall) -> Value {
    serde_json::from_str(&call.arguments).unwrap_or_default()
}

#[test]
fn json_tool_block_becomes_a_call_and_prose_is_kept() {
    let parsed = parse(
        "I'll list the files.\n{\"tool\": \"exec_command\", \"input\": {\"cmd\": \"ls\"}}",
        &tools(),
    );
    assert_eq!(parsed.text, "I'll list the files.");
    assert_eq!(parsed.calls.len(), 1);
    assert_eq!(parsed.calls[0].name, "exec_command");
    assert_eq!(args(&parsed.calls[0]), json!({"cmd": "ls"}));
    assert!(parsed.errors.is_empty());
}

#[test]
fn fenced_arrays_and_name_arguments_shapes_are_accepted() {
    let text = "```json\n[{\"tool\":\"exec_command\",\"input\":{\"cmd\":\"pwd\"}},\
                {\"name\":\"update_plan\",\"parameters\":{\"plan\":[]}}]\n```\nDone.";
    let parsed = parse(text, &tools());
    let names: Vec<&str> = parsed.calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["exec_command", "update_plan"]);
    assert_eq!(parsed.text, "Done.");
}

#[test]
fn structured_envelope_yields_message_and_calls() {
    let parsed = parse(
        r#"{"message": "Checking.", "tool_calls": [{"tool": "exec_command", "input": {"cmd": "git status"}}]}"#,
        &tools(),
    );
    assert_eq!(parsed.text, "Checking.");
    assert_eq!(args(&parsed.calls[0]), json!({"cmd": "git status"}));

    let answer = parse(
        r#"{"message": "All tests pass.", "tool_calls": []}"#,
        &tools(),
    );
    assert_eq!(answer.text, "All tests pass.");
    assert!(answer.calls.is_empty());
}

#[test]
fn legacy_tool_names_map_to_current_tools() {
    let parsed = parse(
        r#"{"tool": "PowerShell", "input": {"cmd": "Get-ChildItem"}}"#,
        &tools(),
    );
    assert_eq!(parsed.calls[0].name, "exec_command");
    let parsed = parse(
        r#"{"tool": "EXEC_COMMAND", "input": {"cmd": "x"}}"#,
        &tools(),
    );
    assert_eq!(parsed.calls[0].name, "exec_command");
}

#[test]
fn unknown_tools_are_dispatched_so_the_model_sees_the_error() {
    let parsed = parse(
        r#"{"tool": "DeleteEverything", "input": {"x": 1}}"#,
        &tools(),
    );
    assert_eq!(
        parsed.calls,
        vec![TextCall {
            name: "DeleteEverything".to_string(),
            arguments: r#"{"x":1}"#.to_string(),
        }]
    );
    assert!(parsed.errors.is_empty());
}

#[test]
fn legacy_file_tools_map_to_shell_and_patch() {
    let read = parse(
        r#"{"tool": "ReadFile", "input": {"path": "src/lib.rs", "start_line": 3, "end_line": "9"}}"#,
        &tools(),
    );
    assert_eq!(read.calls[0].name, "exec_command");
    if !cfg!(windows) {
        assert_eq!(
            args(&read.calls[0])["cmd"],
            json!("sed -n '3,9p' -- 'src/lib.rs'")
        );
    }

    let write = parse(
        r#"{"tool": "WriteFile", "input": {"file_path": "a.txt", "content": "hi\nthere"}}"#,
        &tools(),
    );
    assert_eq!(
        args(&write.calls[0]),
        json!({"input": "*** Begin Patch\n*** Add File: a.txt\n+hi\n+there\n*** End Patch\n"})
    );

    let edit = parse(
        r#"{"tool": "edit_file", "input": {"path": "a.txt", "old_string": "hi", "new_string": "hello"}}"#,
        &tools(),
    );
    assert_eq!(
        args(&edit.calls[0]),
        json!({"input": "*** Begin Patch\n*** Update File: a.txt\n@@\n-hi\n+hello\n*** End Patch\n"})
    );

    let missing = parse(r#"{"tool": "ReadFile", "input": {}}"#, &tools());
    assert!(missing.calls.is_empty());
    assert_eq!(
        missing.errors,
        vec!["`ReadFile` needs a `path`.".to_string()]
    );
}

#[test]
fn shell_blocks_carry_descriptions_when_the_tool_accepts_them() {
    let parsed = parse(
        "<powershell description=\"Create the build folder\">New-Item -ItemType Directory build</powershell>",
        &tools(),
    );
    assert_eq!(
        args(&parsed.calls[0]),
        json!({"cmd": "New-Item -ItemType Directory build", "description": "Create the build folder"})
    );
    let bash = parse("<bash>cargo test</bash>", &tools());
    assert_eq!(args(&bash.calls[0]), json!({"cmd": "cargo test"}));
}

#[test]
fn write_file_becomes_an_add_file_patch() {
    let parsed = parse(
        "<write_file path=\"notes.txt\">\nline one\nline two\n</write_file>",
        &tools(),
    );
    assert_eq!(parsed.calls[0].name, "apply_patch");
    assert_eq!(
        args(&parsed.calls[0]),
        json!({"input": "*** Begin Patch\n*** Add File: notes.txt\n+line one\n+line two\n*** End Patch\n"})
    );
}

#[test]
fn edit_file_becomes_an_update_patch() {
    let parsed = parse(
        "<edit_file path=\"calc.py\"><old>    return a - b</old><new>    return a + b</new></edit_file>",
        &tools(),
    );
    assert_eq!(
        args(&parsed.calls[0]),
        json!({"input": "*** Begin Patch\n*** Update File: calc.py\n@@\n-    return a - b\n+    return a + b\n*** End Patch\n"})
    );
}

#[test]
fn read_file_becomes_a_read_only_command() {
    let parsed = parse(
        "<read_file path=\"src/main.rs\" start=\"10\" end=\"20\"/>",
        &tools(),
    );
    let cmd = args(&parsed.calls[0])["cmd"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if cfg!(windows) {
        assert_eq!(
            cmd,
            "Get-Content -LiteralPath 'src/main.rs' | Select-Object -Skip 9 -First 11"
        );
    } else {
        assert_eq!(cmd, "sed -n '10,20p' -- 'src/main.rs'");
    }
    let whole = parse("<read_file>it's.txt</read_file>", &tools());
    let cmd = args(&whole.calls[0])["cmd"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if !cfg!(windows) {
        assert_eq!(cmd, "cat -- 'it'\\''s.txt'");
    }
}

#[test]
fn patch_json_input_strings_are_wrapped() {
    let parsed = parse(
        r#"{"tool": "apply_patch", "input": "*** Begin Patch\n*** End Patch"}"#,
        &tools(),
    );
    assert_eq!(
        args(&parsed.calls[0]),
        json!({"input": "*** Begin Patch\n*** End Patch"})
    );
}

#[test]
fn deepseek_sentinel_tokens_are_understood() {
    let text = "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>exec_command\n```json\n{\"cmd\": \"ls\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>";
    let parsed = parse(text, &tools());
    assert_eq!(parsed.calls.len(), 1);
    assert_eq!(args(&parsed.calls[0]), json!({"cmd": "ls"}));
    assert_eq!(parsed.text, "");
}

#[test]
fn ordinary_json_in_prose_is_left_alone() {
    let text = "The config looks like {\"port\": 8080}.";
    let parsed = parse(text, &tools());
    assert!(parsed.calls.is_empty());
    assert_eq!(parsed.text, text);
}

#[test]
fn protocol_instructions_list_tools_compactly() {
    let text = protocol_instructions(&tools(), TextToolFormat::Json);
    assert!(text.contains("{\"tool\": \"<tool name>\""));
    assert!(text.contains(
        "- exec_command(cmd: string, description?: string, workdir?: string): Runs a command."
    ));
    assert!(text.contains("- apply_patch(input: string): Edit files."));
    let xml = protocol_instructions(&tools(), TextToolFormat::Xml);
    assert!(xml.contains("<write_file path="));
}

#[test]
fn structured_schema_enumerates_tool_names() {
    let schema = structured_output_schema(&tools());
    assert_eq!(
        schema["properties"]["tool_calls"]["items"]["properties"]["tool"]["enum"],
        json!(["exec_command", "apply_patch", "update_plan"])
    );
}
