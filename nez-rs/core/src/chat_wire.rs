//! Request options for providers that speak chat completions or Ollama's API.
//!
//! The Responses path gets everything it needs from `ModelInfo`. Chat
//! providers additionally need per-model sampling and, for Ollama, a context
//! length sized to what the model supports. This module combines the model
//! profile, the server-reported capabilities and the effective context window.

use nez_api::chat::ChatDialect;
use nez_api::chat::ChatOptions;
use nez_api::chat::ModelCapabilities;
use nez_model_profiles::ModelProfile;
use nez_model_profiles::ToolMode;
use nez_model_provider_info::WireApi;
use nez_protocol::openai_models::ModelInfo;

/// The chat dialect for a wire API, or `None` for the Responses API.
pub(crate) fn chat_dialect(wire_api: WireApi) -> Option<ChatDialect> {
    match wire_api {
        WireApi::Responses => None,
        WireApi::Chat => Some(ChatDialect::OpenAi),
        WireApi::Ollama => Some(ChatDialect::Ollama),
    }
}

/// Inputs that shape a chat request besides the conversation itself.
#[derive(Debug, Clone)]
pub(crate) struct ChatRequestContext<'a> {
    pub dialect: ChatDialect,
    pub model_info: &'a ModelInfo,
    pub profile: &'a ModelProfile,
    pub capabilities: &'a ModelCapabilities,
    /// Use the text tool protocol even for models with native tool support.
    pub force_text_tools: bool,
}

/// `NEZ_DISABLE_NATIVE_TOOLS=1` switches every model to the text protocol,
/// for models whose native tool template misbehaves.
pub(crate) fn native_tools_disabled_by_env() -> bool {
    std::env::var("NEZ_DISABLE_NATIVE_TOOLS")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
}

pub(crate) fn chat_options(ctx: &ChatRequestContext<'_>) -> ChatOptions {
    let mut options = ChatOptions::new(ctx.dialect);
    options.temperature = Some(ctx.profile.sampling.temperature);
    options.top_p = Some(ctx.profile.sampling.top_p);
    if ctx.dialect == ChatDialect::Ollama {
        options.num_ctx = Some(effective_context_tokens(ctx));
    }
    match ctx
        .profile
        .resolve_tool_calling(ctx.capabilities.tools, ctx.force_text_tools)
    {
        ToolMode::Native => {}
        ToolMode::Text(format) => {
            options.native_tools = false;
            options.text_tools = Some(format);
            options.structured_output = ctx.profile.structured_output;
        }
    }
    options
}

/// Context length to request from Ollama: the configured window (profile
/// default unless `model_context_window` overrides it), capped at what the
/// model reports. Ollama otherwise truncates prompts at its own small default.
pub(crate) fn effective_context_tokens(ctx: &ChatRequestContext<'_>) -> u32 {
    let window = ctx.model_info.context_window.filter(|w| *w > 0);
    // Models without a profile carry the large hosted fallback; asking Ollama
    // for that much context would not fit on a consumer GPU.
    let window = window.filter(|w| {
        *w != nez_models_manager::model_info::FALLBACK_CONTEXT_WINDOW || ctx.profile.id != "generic"
    });
    let configured = window
        .and_then(|w| u32::try_from(w).ok())
        .unwrap_or(ctx.profile.context_tokens);
    match ctx.capabilities.context_length {
        Some(max) if max > 0 => configured.min(max),
        _ => configured,
    }
}

#[cfg(test)]
#[path = "chat_wire_tests.rs"]
mod tests;
