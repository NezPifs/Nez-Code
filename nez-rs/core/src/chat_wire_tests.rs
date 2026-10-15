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
    });
    assert_eq!(options.num_ctx, None);
    assert_eq!(options.temperature, Some(0.2));
}

#[test]
fn unknown_local_models_use_a_modest_context() {
    let options = ctx_options("some-new-model", ModelCapabilities::default(), None);
    assert_eq!(options.num_ctx, Some(8_192));
}
