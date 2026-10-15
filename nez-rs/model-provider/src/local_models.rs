//! Model catalog for chat-completions and Ollama providers.
//!
//! The bundled catalog describes OpenAI's hosted models, which a local or
//! third-party server does not serve. This manager lists what the server
//! actually has (`/api/tags` for Ollama, `/models` for OpenAI-compatible
//! servers) and describes each model with its profile.

use std::sync::Arc;
use std::time::Duration;

use nez_http_client::ClientRouteClass;
use nez_http_client::HttpClientFactory;
use nez_login::AuthManager;
use nez_login::default_client::ClientRedirectPolicy;
use nez_login::default_client::create_client_for_route_async;
use nez_model_provider_info::DEEPSEEK_BASE_URL;
use nez_model_provider_info::ModelProviderInfo;
use nez_model_provider_info::WireApi;
use nez_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use nez_models_manager::manager::ModelsManager;
use nez_models_manager::manager::ModelsManagerFuture;
use nez_models_manager::manager::RefreshStrategy;
use nez_models_manager::model_info::model_info_from_slug;
use nez_protocol::config_types::CollaborationModeMask;
use nez_protocol::openai_models::ModelInfo;
use nez_protocol::openai_models::ModelVisibility;
use nez_protocol::openai_models::ModelsResponse;
use serde_json::Value;
use tokio::sync::RwLock;
use tokio::sync::TryLockError;

const LIST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct LocalModelsManager {
    info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
    models: RwLock<Option<Vec<ModelInfo>>>,
}

impl LocalModelsManager {
    pub(crate) fn new(info: ModelProviderInfo, auth_manager: Option<Arc<AuthManager>>) -> Self {
        Self {
            info,
            auth_manager,
            models: RwLock::new(None),
        }
    }

    async fn fetch(&self, http_client_factory: HttpClientFactory) -> Option<Vec<ModelInfo>> {
        let base_url = self.info.base_url.clone()?;
        let (url, ollama) = match self.info.wire_api {
            WireApi::Ollama => (
                format!("{}/api/tags", nez_api::chat::ollama_root(&base_url)),
                true,
            ),
            _ => (format!("{}/models", base_url.trim_end_matches('/')), false),
        };
        let client = create_client_for_route_async(
            http_client_factory,
            url.clone(),
            ClientRouteClass::Api,
            ClientRedirectPolicy::Default,
        )
        .await
        .ok()?;
        let mut request = client.get(&url).timeout(LIST_TIMEOUT);
        if let Ok(Some(key)) = self.info.api_key() {
            request = request.bearer_auth(key);
        }
        let body: Value = match request
            .send()
            .await
            .and_then(nez_http_client::HttpResponse::error_for_status)
        {
            Ok(response) => response.json().await.ok()?,
            Err(err) => {
                tracing::debug!("listing models from {url} failed: {err}");
                return None;
            }
        };
        Some(catalog_from_listing(&body, ollama))
    }

    /// Models to offer when the server cannot be reached.
    fn fallback(&self) -> Vec<ModelInfo> {
        let is_deepseek = self
            .info
            .base_url
            .as_deref()
            .is_some_and(|url| url.starts_with(DEEPSEEK_BASE_URL.trim_end_matches("/v1")));
        let slugs: &[&str] = if is_deepseek {
            &["deepseek-chat", "deepseek-reasoner"]
        } else if self.info.wire_api == WireApi::Ollama {
            &[nez_model_profiles::DEFAULT_LOCAL_MODEL]
        } else {
            &[]
        };
        slugs
            .iter()
            .enumerate()
            .map(|(i, slug)| listed_model(slug, None, i))
            .collect()
    }

    async fn current(&self) -> Vec<ModelInfo> {
        self.models
            .read()
            .await
            .clone()
            .unwrap_or_else(|| self.fallback())
    }
}

fn listed_model(slug: &str, description: Option<String>, index: usize) -> ModelInfo {
    let mut info = model_info_from_slug(slug);
    info.visibility = ModelVisibility::List;
    info.description = description;
    info.priority = i32::try_from(index).unwrap_or(i32::MAX);
    info.used_fallback_model_metadata = false;
    info
}

/// Parses an Ollama `/api/tags` or OpenAI `/models` listing.
pub(crate) fn catalog_from_listing(body: &Value, ollama: bool) -> Vec<ModelInfo> {
    let entries = if ollama {
        body.get("models")
    } else {
        body.get("data")
    };
    let mut models: Vec<(String, Option<String>)> = entries
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let slug = entry
                .get(if ollama { "name" } else { "id" })
                .and_then(Value::as_str)?
                .to_string();
            // Embedding models cannot chat.
            if slug.contains("embed") {
                return None;
            }
            let details = entry.get("details");
            let field = |key: &str| {
                details
                    .and_then(|d| d.get(key))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            };
            let description = match (field("parameter_size"), field("quantization_level")) {
                (Some(size), Some(quant)) => Some(format!("{size} · {quant}")),
                (Some(size), None) => Some(size.to_string()),
                _ => None,
            };
            Some((slug, description))
        })
        .collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    models
        .into_iter()
        .enumerate()
        .map(|(i, (slug, description))| listed_model(&slug, description, i))
        .collect()
}

impl ModelsManager for LocalModelsManager {
    fn raw_model_catalog(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, ModelsResponse> {
        Box::pin(async move {
            let cached = self.models.read().await.is_some();
            let refresh = match refresh_strategy {
                RefreshStrategy::Online => true,
                RefreshStrategy::OnlineIfUncached => !cached,
                RefreshStrategy::Offline => false,
            };
            if refresh && let Some(models) = self.fetch(http_client_factory).await {
                *self.models.write().await = Some(models);
            }
            ModelsResponse {
                models: self.current().await,
            }
        })
    }

    fn get_remote_models(&self) -> ModelsManagerFuture<'_, Vec<ModelInfo>> {
        Box::pin(self.current())
    }

    fn try_get_remote_models(&self) -> Result<Vec<ModelInfo>, TryLockError> {
        Ok(self
            .models
            .try_read()?
            .clone()
            .unwrap_or_else(|| self.fallback()))
    }

    fn auth_manager(&self) -> Option<&AuthManager> {
        self.auth_manager.as_deref()
    }

    fn list_collaboration_modes(&self) -> Vec<CollaborationModeMask> {
        builtin_collaboration_mode_presets()
    }

    fn refresh_if_new_etag(
        &self,
        _etag: String,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsManagerFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[cfg(test)]
#[path = "local_models_tests.rs"]
mod tests;
