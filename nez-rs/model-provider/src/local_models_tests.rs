use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn slugs(models: &[ModelInfo]) -> Vec<&str> {
    models.iter().map(|m| m.slug.as_str()).collect()
}

#[test]
fn ollama_tags_become_listed_models() {
    let body = json!({"models": [
        {"name": "qwen3.5:9b", "details": {"parameter_size": "9.7B", "quantization_level": "Q4_K_M"}},
        {"name": "nomic-embed-text:latest", "details": {}},
        {"name": "deepseek-coder-v2:16b", "details": {"parameter_size": "15.7B"}}
    ]});
    let models = catalog_from_listing(&body, /*ollama*/ true);
    assert_eq!(slugs(&models), vec!["deepseek-coder-v2:16b", "qwen3.5:9b"]);
    assert_eq!(models[1].description.as_deref(), Some("9.7B · Q4_K_M"));
    assert_eq!(models[0].description.as_deref(), Some("15.7B"));
    assert!(models.iter().all(|m| m.visibility == ModelVisibility::List));
    assert_eq!(models[1].context_window, Some(16_384));
}

#[test]
fn openai_style_listings_use_ids() {
    let body =
        json!({"object": "list", "data": [{"id": "deepseek-reasoner"}, {"id": "deepseek-chat"}]});
    assert_eq!(
        slugs(&catalog_from_listing(&body, /*ollama*/ false)),
        vec!["deepseek-chat", "deepseek-reasoner"]
    );
}

#[test]
fn unreachable_servers_fall_back_to_known_models() {
    let ollama = LocalModelsManager::new(nez_model_provider_info::create_ollama_provider(), None);
    assert_eq!(slugs(&ollama.fallback()), vec!["qwen3.5:9b"]);
    let deepseek =
        LocalModelsManager::new(nez_model_provider_info::create_deepseek_provider(), None);
    assert_eq!(
        slugs(&deepseek.fallback()),
        vec!["deepseek-chat", "deepseek-reasoner"]
    );
}
