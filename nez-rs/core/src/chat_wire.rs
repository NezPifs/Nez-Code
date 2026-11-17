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
use nez_protocol::models::ContentItem;
use nez_protocol::models::ResponseItem;
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
    pub vision: VisionMode,
    /// Let thinking models think. Off by default for agent turns: small local
    /// models can spend the whole output budget reasoning and never act.
    pub enable_thinking: bool,
    /// The user set the reasoning effort to `none`.
    pub reasoning_disabled: bool,
}

/// `NEZ_VISION`: whether images are sent to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisionMode {
    /// Send images unless the server reports the model is text-only.
    Auto,
    On,
    Off,
}

impl VisionMode {
    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var("NEZ_VISION").ok().as_deref())
    }

    fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("on" | "1" | "true" | "yes") => Self::On,
            Some("off" | "0" | "false" | "no") => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// `NEZ_ENABLE_THINKING=1` restores the model's own thinking behaviour.
pub(crate) fn thinking_enabled_by_env() -> bool {
    std::env::var("NEZ_ENABLE_THINKING")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
}

fn images_allowed(vision: VisionMode, capabilities: &ModelCapabilities) -> bool {
    match vision {
        VisionMode::On => true,
        VisionMode::Off => false,
        // Servers without capability metadata keep the images.
        VisionMode::Auto => capabilities.vision != Some(false),
    }
}

/// An error for a prompt with images sent to a model the server reports as
/// text-only. Older images in history are dropped with a note instead, so one
/// screenshot does not break the rest of the session.
pub(crate) fn text_only_image_error(
    model: &str,
    vision: VisionMode,
    capabilities: &ModelCapabilities,
    input: &[ResponseItem],
) -> Option<String> {
    if vision != VisionMode::Auto
        || capabilities.vision != Some(false)
        || !latest_user_message_has_images(input)
    {
        return None;
    }
    Some(format!(
        "`{model}` cannot read images: Ollama reports it as a text-only model. Switch to a vision \
         model such as `qwen2.5vl:7b`, or set NEZ_VISION=on to send images anyway."
    ))
}

fn latest_user_message_has_images(input: &[ResponseItem]) -> bool {
    input
        .iter()
        .rev()
        .find_map(|item| match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                let is_context = content.iter().all(|part| {
                    matches!(part, ContentItem::InputText { text } if text.trim_start().starts_with('<'))
                });
                (!is_context).then(|| {
                    content
                        .iter()
                        .any(|part| matches!(part, ContentItem::InputImage { .. }))
                })
            }
            _ => None,
        })
        .unwrap_or(false)
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
    options.allow_images = images_allowed(ctx.vision, ctx.capabilities);
    if ctx.dialect == ChatDialect::Ollama {
        options.num_ctx = Some(effective_context_tokens(ctx));
        options.lean_tools = true;
        // `think: false` is ignored by models that do not think.
        if !ctx.enable_thinking {
            options.think = Some(false);
        }
    }
    if ctx.reasoning_disabled {
        options.think = Some(false);
        // An explicit choice, so forward it to OpenAI-compatible servers too.
        options.send_reasoning_effort = true;
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
