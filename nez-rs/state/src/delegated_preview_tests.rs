//! Verify delegated preview decoding and tool identity filtering.

use super::delegated_output_preview;
use nez_protocol::items::FunctionCallOutputItem;
use nez_protocol::models::FunctionCallOutputBody;
use pretty_assertions::assert_eq;

#[test]
fn delegated_previews_unwrap_only_recognized_inputs() {
    let wrapped = "<nez_delegation>\n  <source_thread_id>source</source_thread_id>\n  <input>Check &lt;main&gt; &amp; &amp;lt;literal&amp;gt;</input>\n</nez_delegation>";
    for (namespace, name, text, expected) in [
        (
            Some("nez_app"),
            "create_thread",
            wrapped,
            Some("Check <main> & &lt;literal&gt;"),
        ),
        (
            Some("nez_tui"),
            "send_message_to_thread",
            wrapped,
            Some("Check <main> & &lt;literal&gt;"),
        ),
        (
            Some("nez_app"),
            "create_thread",
            "plain &lt;text&gt;",
            Some("plain &lt;text&gt;"),
        ),
        (Some("nez_tui"), "create_thread", "  ", None),
        (Some("other"), "create_thread", wrapped, None),
        (None, "create_thread", wrapped, None),
        (Some("nez_app"), "shell", wrapped, None),
        (
            Some("nez_app"),
            "create_thread",
            "<nez_delegation>incomplete",
            Some("<nez_delegation>incomplete"),
        ),
    ] {
        let output = FunctionCallOutputItem {
            id: "output".to_string(),
            name: name.to_string(),
            namespace: namespace.map(str::to_string),
            output: FunctionCallOutputBody::Text(text.to_string()),
        };
        assert_eq!(delegated_output_preview(&output).as_deref(), expected);
    }
}
