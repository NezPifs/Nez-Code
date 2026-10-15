use super::*;
use nez_model_profiles::profile_for_model;
use nez_models_manager::model_info::model_info_from_slug;
use pretty_assertions::assert_eq;

fn ctx_options(model: &str, caps: ModelCapabilities, window: Option<i64>) -> ChatOptions {
    let mut info = model_info_from_slug(model);
    if let Some(window) = window {
        info.context_window = Some(window);
    }
    let profile = profile_for_model(model);
    chat_options(&ChatRequestContext {
        dialect: ChatDialect::Ollama,
        model_info: &info,
        profile: &profile,
        capabilities: &caps,
        force_text_tools: false,
        reasoning_disabled: false,
    })
}

#[test]
fn wire_apis_map_to_dialects() {
    assert_eq!(chat_dialect(WireApi::Responses), None);
    assert_eq!(chat_dialect(WireApi::Chat), Some(ChatDialect::OpenAi));
    assert_eq!(chat_dialect(WireApi::Ollama), Some(ChatDialect::Ollama));
}

#[test]
fn ollama_requests_use_profile_sampling_and_context() {
    let options = ctx_options("qwen3.5:9b", ModelCapabilities::default(), None);
    assert_eq!(options.num_ctx, Some(16_384));
    assert_eq!(options.temperature, Some(0.2));
    assert_eq!(options.top_p, Some(0.9));
}

#[test]
fn context_is_capped_at_the_model_maximum() {
    let caps = ModelCapabilities {
        context_length: Some(8_192),
        ..Default::default()
    };
    assert_eq!(ctx_options("qwen3.5:9b", caps, None).num_ctx, Some(8_192));
}

#[test]
fn configured_context_window_overrides_the_profile() {
    let caps = ModelCapabilities {
        context_length: Some(262_144),
        ..Default::default()
    };
    assert_eq!(
        ctx_options("qwen3.5:9b", caps, Some(32_768)).num_ctx,
        Some(32_768)
    );
}

#[test]
fn hosted_chat_requests_do_not_send_num_ctx() {
    let info = model_info_from_slug("deepseek-chat");
    let profile = profile_for_model("deepseek-chat");
    let options = chat_options(&ChatRequestContext {
        dialect: ChatDialect::OpenAi,
        model_info: &info,
        profile: &profile,
        capabilities: &ModelCapabilities::default(),
        force_text_tools: false,
        reasoning_disabled: false,
    });
    assert_eq!(options.num_ctx, None);
    assert_eq!(options.temperature, Some(0.2));
    assert_eq!(options.think, None);
}

#[test]
fn reasoning_effort_none_disables_thinking_on_both_dialects() {
    let info = model_info_from_slug("deepseek-chat");
    let profile = profile_for_model("deepseek-chat");
    for dialect in [ChatDialect::OpenAi, ChatDialect::Ollama] {
        let options = chat_options(&ChatRequestContext {
            dialect,
            model_info: &info,
            profile: &profile,
            capabilities: &ModelCapabilities::default(),
            force_text_tools: false,
            reasoning_disabled: true,
        });
        assert_eq!(options.think, Some(false));
        assert!(options.send_reasoning_effort);
    }
}

#[test]
fn unknown_local_models_use_a_modest_context() {
    let options = ctx_options("some-new-model", ModelCapabilities::default(), None);
    assert_eq!(options.num_ctx, Some(8_192));
}

#[test]
fn small_models_use_the_text_protocol_with_structured_output() {
    let options = ctx_options(
        "qwen2.5-coder:7b-instruct",
        ModelCapabilities::default(),
        None,
    );
    assert_eq!(
        (
            options.native_tools,
            options.text_tools,
            options.structured_output
        ),
        (false, Some(nez_model_profiles::TextToolFormat::Json), true)
    );
}

#[test]
fn native_models_keep_native_tools_unless_forced() {
    let options = ctx_options("qwen3.5:9b", ModelCapabilities::default(), None);
    assert_eq!((options.native_tools, options.text_tools), (true, None));

    let info = model_info_from_slug("qwen3.5:9b");
    let profile = profile_for_model("qwen3.5:9b");
    let forced = chat_options(&ChatRequestContext {
        dialect: ChatDialect::Ollama,
        model_info: &info,
        profile: &profile,
        capabilities: &ModelCapabilities::default(),
        force_text_tools: true,
        reasoning_disabled: false,
    });
    assert_eq!(forced.native_tools, false);
    assert!(forced.text_tools.is_some());
}

#[test]
fn auto_profiles_follow_the_server_tool_capability() {
    let with_tools = ModelCapabilities {
        tools: Some(true),
        ..Default::default()
    };
    assert_eq!(
        ctx_options("llama3.1:8b", with_tools, None).text_tools,
        None
    );
    assert!(
        ctx_options("llama3.1:8b", ModelCapabilities::default(), None)
            .text_tools
            .is_some()
    );
}
