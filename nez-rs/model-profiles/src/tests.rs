use super::*;
use pretty_assertions::assert_eq;

#[test]
fn default_model_uses_native_tools_and_a_16k_context() {
    let profile = profile_for_model(DEFAULT_LOCAL_MODEL);
    assert_eq!(profile.id, "qwen-9b");
    assert_eq!(profile.tool_calling, ToolCalling::Native);
    assert_eq!(profile.context_tokens, 16_384);
    assert_eq!(profile.sampling, AGENT_SAMPLING);
    assert!(!profile.structured_output);
}

#[test]
fn qwen_sizes_match_whole_tokens() {
    // `27b` must not match the `7b` rule and `14b` must not match `4b`.
    assert_eq!(profile_for_model("qwen3:27b").id, "qwen-large");
    assert_eq!(profile_for_model("qwen3:14b").id, "qwen-large");
    assert_eq!(profile_for_model("qwen3:4b").id, "qwen-small");
    assert_eq!(profile_for_model("qwen3:8b").id, "qwen-small");
    assert_eq!(
        profile_for_model("qwen2.5-coder:7b-instruct").id,
        "qwen-small"
    );
    assert_eq!(profile_for_model("qwen2.5:0.5b").id, "qwen-small");
}

#[test]
fn qwen_moe_and_untagged_builds_use_the_large_profile() {
    assert_eq!(profile_for_model("qwen3.6:35b-a3b").id, "qwen-large");
    assert_eq!(profile_for_model("qwen3-coder-next").id, "qwen-large");
    assert_eq!(
        profile_for_model("hf.co/Qwen/Qwen3-14B-GGUF:Q4_K_M").id,
        "qwen-large"
    );
}

#[test]
fn small_qwen_uses_json_text_tools_with_structured_output() {
    let profile = profile_for_model("qwen2.5-coder:7b-instruct");
    assert_eq!(
        profile.tool_calling,
        ToolCalling::Text(TextToolFormat::Json)
    );
    assert_eq!(profile.context_tokens, 8_192);
    assert!(profile.structured_output);
    assert!(profile.code_specialist);
}

#[test]
fn deepseek_coder_is_a_json_code_specialist() {
    let profile = profile_for_model("deepseek-coder-v2:16b");
    assert_eq!(profile.id, "deepseek-coder");
    assert_eq!(
        profile.tool_calling,
        ToolCalling::Text(TextToolFormat::Json)
    );
    assert!(profile.code_specialist);
    assert_eq!(profile.context_tokens, 16_384);
}

#[test]
fn hosted_deepseek_uses_native_tools() {
    let profile = profile_for_model("deepseek-chat");
    assert_eq!(profile.id, "deepseek-api");
    assert_eq!(profile.tool_calling, ToolCalling::Native);
    assert!(!profile.structured_output);
}

#[test]
fn other_families_follow_server_capabilities() {
    for model in ["llama3.1:8b", "mistral:7b", "gemma3:12b", "phi4:14b"] {
        assert_eq!(profile_for_model(model).id, "json-tools", "{model}");
    }
    assert_eq!(profile_for_model("some-new-model").id, "generic");
}

#[test]
fn tool_calling_resolution_honours_server_and_kill_switch() {
    let native = profile_for_model("qwen3.5:9b");
    assert_eq!(native.resolve_tool_calling(None, false), ToolMode::Native);
    assert_eq!(
        native.resolve_tool_calling(Some(true), false),
        ToolMode::Native
    );
    assert_eq!(
        native.resolve_tool_calling(Some(false), false),
        ToolMode::Text(TextToolFormat::Json)
    );
    assert_eq!(
        native.resolve_tool_calling(Some(true), true),
        ToolMode::Text(TextToolFormat::Json)
    );

    let auto = profile_for_model("llama3.1:8b");
    assert_eq!(
        auto.resolve_tool_calling(Some(true), false),
        ToolMode::Native
    );
    assert_eq!(
        auto.resolve_tool_calling(None, false),
        ToolMode::Text(TextToolFormat::Json)
    );

    let text = profile_for_model("deepseek-coder-v2:16b");
    assert_eq!(
        text.resolve_tool_calling(Some(true), false),
        ToolMode::Text(TextToolFormat::Json)
    );
}

#[test]
fn vram_tiers_pick_the_expected_models() {
    let pick = |gib: u64| recommended_model_for_vram(Some(gib * 1024)).model;
    assert_eq!(pick(4), "qwen2.5-coder:7b-instruct");
    assert_eq!(pick(6), "qwen2.5-coder:7b-instruct");
    assert_eq!(pick(8), "qwen3.5:9b");
    assert_eq!(pick(11), "qwen3.5:9b");
    assert_eq!(pick(12), "deepseek-coder-v2:16b");
    assert_eq!(pick(24), "deepseek-coder-v2:16b");
    // An 8 GB card usually reports a little under 8 GiB.
    assert_eq!(recommended_model_for_vram(Some(7_800)).model, "qwen3.5:9b");
    assert_eq!(recommended_model_for_vram(None).model, DEFAULT_LOCAL_MODEL);
}

#[test]
fn hosted_openai_style_names_keep_a_large_context() {
    let profile = profile_for_model("gpt-5.5-mini");
    assert_eq!(profile.id, "openai-hosted");
    assert_eq!(profile.context_tokens, 272_000);
    assert_eq!(profile_for_model("o4-mini").id, "openai-hosted");
    assert_eq!(profile_for_model("gpt-oss:20b").id, "gpt-oss");
}
