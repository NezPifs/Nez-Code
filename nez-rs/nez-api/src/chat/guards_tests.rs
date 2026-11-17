use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn user(text: &str) -> Value {
    json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]})
}

fn call(id: &str, cmd: &str) -> Value {
    json!({"type": "function_call", "name": "exec_command", "call_id": id,
           "arguments": json!({"cmd": cmd}).to_string()})
}

fn output(id: &str, text: &str) -> Value {
    json!({"type": "function_call_output", "call_id": id, "output": text})
}

#[test]
fn repeated_identical_outputs_keep_only_the_latest() {
    let long = "x".repeat(300);
    let mut input = vec![
        user("check it"),
        call("a", "cat big.txt"),
        output("a", &long),
        call("b", "cat big.txt"),
        output("b", &long),
        call("c", "cat other.txt"),
        output("c", &long),
    ];
    collapse_duplicate_results(&mut input);
    assert_eq!(input[2]["output"], COLLAPSED_OUTPUT);
    assert_eq!(input[4]["output"], long.as_str());
    // Same text from a different call is not a duplicate.
    assert_eq!(input[6]["output"], long.as_str());
}

#[test]
fn short_and_image_outputs_are_left_alone() {
    let image_output = json!([{"type": "input_image", "image_url": "data:image/png;base64,AA"}]);
    let mut input = vec![
        call("a", "ls"),
        output("a", "ok"),
        call("b", "ls"),
        output("b", "ok"),
        call("c", "shot"),
        json!({"type": "function_call_output", "call_id": "c", "output": image_output}),
        call("d", "shot"),
        json!({"type": "function_call_output", "call_id": "d", "output": image_output}),
    ];
    let before = input.clone();
    collapse_duplicate_results(&mut input);
    assert_eq!(input, before);
}

#[test]
fn goal_is_restated_after_many_tool_calls() {
    let mut input = vec![
        user("<environment_context>cwd</environment_context>"),
        user("Add a --verbose flag to the CLI"),
    ];
    for i in 0..GOAL_REMINDER_AFTER - 1 {
        let id = format!("c{i}");
        input.push(call(&id, &format!("step {i}")));
        input.push(output(&id, "ok"));
    }
    assert_eq!(goal_reminder(&input), None);
    input.push(call("last", "step"));
    input.push(output("last", "ok"));
    let reminder = goal_reminder(&input).unwrap_or_default();
    assert!(
        reminder.contains("Add a --verbose flag to the CLI"),
        "{reminder}"
    );
}

#[test]
fn a_new_user_message_resets_the_reminder() {
    let mut input = vec![user("first task")];
    for i in 0..GOAL_REMINDER_AFTER {
        let id = format!("c{i}");
        input.push(call(&id, "x"));
        input.push(output(&id, "ok"));
    }
    input.push(user("second task"));
    assert_eq!(goal_reminder(&input), None);
}

#[test]
fn three_identical_failures_stop_the_turn() {
    let mut input = vec![user("build it")];
    for id in ["a", "b", "c"] {
        input.push(call(id, "make"));
        input.push(output(
            id,
            "make: *** No targets.\nProcess exited with code 2",
        ));
    }
    let stop = repeated_failure(&input).unwrap_or_default();
    assert!(stop.contains("`exec_command` failed 3 times"), "{stop}");
    assert!(stop.contains("No targets"), "{stop}");
}

#[test]
fn varied_or_successful_calls_do_not_trip_the_breaker() {
    let failed = "Process exited with code 1";
    let mut varied = vec![user("build it")];
    for (id, cmd) in [("a", "make"), ("b", "make all"), ("c", "make")] {
        varied.push(call(id, cmd));
        varied.push(output(id, failed));
    }
    assert_eq!(repeated_failure(&varied), None);

    let mut recovered = vec![user("build it")];
    for (id, out) in [
        ("a", failed),
        ("b", failed),
        ("c", "Process exited with code 0"),
    ] {
        recovered.push(call(id, "make"));
        recovered.push(output(id, out));
    }
    assert_eq!(repeated_failure(&recovered), None);

    // Failures from before the latest user message do not count.
    let mut earlier = vec![user("build it")];
    for id in ["a", "b", "c"] {
        earlier.push(call(id, "make"));
        earlier.push(output(id, failed));
    }
    earlier.push(user("try again with the fixed Makefile"));
    assert_eq!(repeated_failure(&earlier), None);
}

#[test]
fn tool_errors_count_as_failures() {
    assert!(looks_failed("apply_patch verification failed: no match"));
    assert!(looks_failed("unsupported call: DeleteEverything"));
    assert!(!looks_failed("all good\nProcess exited with code 0"));
    assert!(!looks_failed("Success. Updated the following files"));
}
