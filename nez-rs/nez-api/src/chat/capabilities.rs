//! Model capability discovery through Ollama's `/api/show`.
//!
//! Ollama reports what a model's template supports (`tools`, `vision`,
//! `thinking`) and the context length it was trained for. Results are cached
//! per server and model for a few minutes so a turn does not pay for an extra
//! round trip on every request, while a freshly pulled model is picked up soon.

use crate::auth::SharedAuthProvider;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use http::HeaderMap;
use http::Method;
use nez_client::HttpTransport;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

const CACHE_TTL: Duration = Duration::from_secs(300);
const SHOW_TIMEOUT: Duration = Duration::from_secs(5);

/// What the server says about a model. `None` means "not reported".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub thinking: Option<bool>,
    pub context_length: Option<u32>,
}

impl ModelCapabilities {
    /// Parses an `/api/show` response body.
    pub fn from_show(body: &Value) -> Self {
        let caps: Option<Vec<&str>> = body
            .get("capabilities")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect());
        let has = |name: &str| caps.as_ref().map(|list| list.contains(&name));
        let context_length = body
            .get("model_info")
            .and_then(Value::as_object)
            .and_then(|info| {
                info.iter()
                    .find(|(key, _)| key.ends_with(".context_length"))
                    .and_then(|(_, value)| value.as_u64())
            })
            .and_then(|n| u32::try_from(n).ok());
        Self {
            tools: has("tools"),
            vision: has("vision"),
            thinking: has("thinking"),
            context_length,
        }
    }
}

type CacheKey = (String, String);

fn cache() -> &'static Mutex<HashMap<CacheKey, (Instant, ModelCapabilities)>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, (Instant, ModelCapabilities)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: &CacheKey) -> Option<ModelCapabilities> {
    let guard = cache().lock().ok()?;
    guard
        .get(key)
        .filter(|(at, _)| at.elapsed() < CACHE_TTL)
        .map(|(_, caps)| caps.clone())
}

fn store(key: CacheKey, caps: ModelCapabilities) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(key, (Instant::now(), caps));
    }
}

/// Root of an Ollama server given a provider base URL such as
/// `http://localhost:11434/v1`.
pub fn ollama_root(base_url: &str) -> String {
    base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

/// Looks up capabilities for `model`, using the cache when it is fresh.
///
/// Returns the default (everything unknown) when the server does not answer
/// in time or has no `/api/show`, so callers can fall back gracefully on
/// custom OpenAI-compatible servers.
pub async fn ollama_capabilities<T: HttpTransport>(
    transport: T,
    provider: &Provider,
    auth: SharedAuthProvider,
    model: &str,
) -> ModelCapabilities {
    let root = ollama_root(&provider.base_url);
    let key = (root.clone(), model.to_string());
    if let Some(caps) = cached(&key) {
        return caps;
    }
    let mut provider = provider.clone();
    provider.base_url = root;
    provider.retry.max_attempts = 1;
    let session = EndpointSession::new(transport, provider, auth);
    let lookup = session.execute(
        Method::POST,
        "/api/show",
        HeaderMap::new(),
        Some(json!({"model": model})),
    );
    let caps = match tokio::time::timeout(SHOW_TIMEOUT, lookup).await {
        Ok(Ok(response)) => match serde_json::from_slice::<Value>(&response.body) {
            Ok(body) => ModelCapabilities::from_show(&body),
            Err(err) => {
                tracing::debug!("unreadable /api/show response for {model}: {err}");
                ModelCapabilities::default()
            }
        },
        Ok(Err(err)) => {
            log_lookup_error(model, &err);
            ModelCapabilities::default()
        }
        Err(_) => {
            tracing::debug!("/api/show for {model} timed out");
            ModelCapabilities::default()
        }
    };
    store(key, caps.clone());
    caps
}

fn log_lookup_error(model: &str, err: &ApiError) {
    tracing::debug!("capability lookup for {model} failed: {err}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_capabilities_and_context_length() {
        let caps = ModelCapabilities::from_show(&json!({
            "capabilities": ["completion", "tools", "thinking"],
            "model_info": {"general.architecture": "qwen35", "qwen35.context_length": 262144}
        }));
        assert_eq!(
            caps,
            ModelCapabilities {
                tools: Some(true),
                vision: Some(false),
                thinking: Some(true),
                context_length: Some(262_144),
            }
        );
    }

    #[test]
    fn missing_capabilities_stay_unknown() {
        assert_eq!(
            ModelCapabilities::from_show(&json!({"modelfile": "FROM x"})),
            ModelCapabilities::default()
        );
    }

    #[test]
    fn ollama_root_strips_the_openai_suffix() {
        assert_eq!(
            ollama_root("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            ollama_root("http://gpu-box:11434/v1/"),
            "http://gpu-box:11434"
        );
        assert_eq!(ollama_root("http://gpu-box:11434"), "http://gpu-box:11434");
    }
}
