use super::ContextualUserFragment;
use nez_prompts::START_INSTRUCTIONS;
use nez_protocol::models::ContentItemKind;
use nez_protocol::protocol::REALTIME_CONVERSATION_CLOSE_TAG;
use nez_protocol::protocol::REALTIME_CONVERSATION_OPEN_TAG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RealtimeStartInstructions;

impl ContextualUserFragment for RealtimeStartInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("realtime_conversation.start_instructions".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            REALTIME_CONVERSATION_OPEN_TAG,
            REALTIME_CONVERSATION_CLOSE_TAG,
        )
    }

    fn body(&self) -> String {
        format!("\n{}\n", START_INSTRUCTIONS.trim())
    }
}
