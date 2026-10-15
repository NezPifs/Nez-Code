mod client;
pub mod hardware;
mod line_buffer;
mod parser;
mod pull;
mod url;

pub use client::OllamaClient;
use nez_core::config::Config;
pub use pull::CliProgressReporter;
pub use pull::PullEvent;
pub use pull::PullProgressReporter;
pub use pull::TuiProgressReporter;
use semver::Version;

/// Built-in local model; the only model Nez-Code downloads on its own.
pub const DEFAULT_OSS_MODEL: &str = nez_model_profiles::DEFAULT_LOCAL_MODEL;

/// Prepare the local Ollama environment before a session starts.
///
/// - Ensures a local Ollama server is reachable (done by the client).
/// - Pulls the built-in default model when it is missing. Other models are
///   never downloaded implicitly; a clear error explains how to install them.
pub async fn ensure_oss_ready(config: &Config, client: &OllamaClient) -> std::io::Result<()> {
    let model = config.model.as_deref().unwrap_or(DEFAULT_OSS_MODEL);
    let models = match client.fetch_models().await {
        Ok(models) => models,
        Err(err) => {
            // Not fatal; the first request reports a precise error.
            tracing::warn!("Failed to query local models from Ollama: {err}.");
            return Ok(());
        }
    };
    if is_installed(&models, model) {
        return Ok(());
    }
    if model == DEFAULT_OSS_MODEL {
        let mut reporter = crate::CliProgressReporter::new();
        return client.pull_with_reporter(model, &mut reporter).await;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "Model `{model}` is not installed in Ollama. Install it with `ollama pull {model}`, or pick an installed model with `/model`."
        ),
    ))
}

/// Ollama lists `name:latest` for models pulled without a tag.
fn is_installed(models: &[String], model: &str) -> bool {
    models.iter().any(|installed| {
        installed == model
            || (!model.contains(':') && installed == &format!("{model}:latest"))
            || installed.strip_suffix(":latest") == Some(model)
    })
}

fn min_supported_version() -> Version {
    // `think` and streamed tool calls on /api/chat.
    Version::new(0, 9, 0)
}

fn is_supported(version: &Version) -> bool {
    *version == Version::new(0, 0, 0) || *version >= min_supported_version()
}

/// Ensure the running Ollama server supports the native chat features
/// Nez-Code relies on.
///
/// Returns `Ok(())` when the version endpoint is missing or unparsable.
pub async fn ensure_server_supported(client: &OllamaClient) -> std::io::Result<()> {
    let Some(version) = client.fetch_version().await? else {
        return Ok(());
    };

    if is_supported(&version) {
        return Ok(());
    }

    let min = min_supported_version();
    Err(std::io::Error::other(format!(
        "Ollama {version} is too old. Nez-Code requires Ollama {min} or newer; update it from https://ollama.com/download."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nez_http_client::HttpClientFactory;
    use nez_http_client::OutboundProxyPolicy;
    use nez_model_provider_info::WireApi;
    use nez_model_provider_info::create_oss_provider_with_base_url;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn version_check_reuses_existing_ollama_client() {
        if std::env::var(nez_core::spawn::NEZ_SANDBOX_NETWORK_DISABLED_ENV_VAR).is_ok() {
            tracing::info!(
                "{} set; skipping version_check_reuses_existing_ollama_client",
                nez_core::spawn::NEZ_SANDBOX_NETWORK_DISABLED_ENV_VAR
            );
            return;
        }

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/tags"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models": [{"name": "qwen3.5:9b"}]})),
            )
            .expect(2)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/version"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"version": "0.14.1"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let provider = create_oss_provider_with_base_url(&server.uri(), WireApi::Responses);
        let client = OllamaClient::try_from_provider(
            &provider,
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await
        .expect("create Ollama client");

        ensure_server_supported(&client)
            .await
            .expect("version check should reuse the existing client");
        assert_eq!(
            client.fetch_models().await.expect("fetch models"),
            vec!["qwen3.5:9b"]
        );

        server.verify().await;
    }

    #[test]
    fn dev_builds_are_supported() {
        assert!(is_supported(&Version::new(0, 0, 0)));
    }

    #[test]
    fn versions_before_native_thinking_are_rejected() {
        assert!(!is_supported(&Version::new(0, 8, 9)));
        assert!(is_supported(&Version::new(0, 9, 0)));
        assert!(is_supported(&Version::new(0, 14, 0)));
    }

    #[test]
    fn untagged_models_match_latest() {
        let installed = vec!["qwen3.5:9b".to_string(), "llama3.1:latest".to_string()];
        assert!(is_installed(&installed, "qwen3.5:9b"));
        assert!(is_installed(&installed, "llama3.1"));
        assert!(is_installed(&installed, "llama3.1:latest"));
        assert!(!is_installed(&installed, "qwen3.5:27b"));
    }
}
