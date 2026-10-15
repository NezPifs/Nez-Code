//! Per-model behaviour for local and hosted chat models.
//!
//! Small local models differ a lot in how reliably they call tools, how much
//! context they can afford and which sampling settings keep them on task. A
//! [`ModelProfile`] captures those differences so the request layer does not
//! have to guess from the model name in several places.
//!
//! Profiles are chosen from the model name alone. Facts reported by the server
//! at runtime (for example Ollama's `/api/show` capabilities) refine the choice
//! through [`ModelProfile::resolve_tool_calling`].

mod tiers;

pub use tiers::DEFAULT_LOCAL_MODEL;
pub use tiers::HardwareTier;
pub use tiers::TIERS;
pub use tiers::recommended_model_for_vram;

use serde::Deserialize;
use serde::Serialize;

/// Text formats a model may use to request tools when it cannot emit native
/// tool calls. The parser accepts both; this decides which one the prompt teaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextToolFormat {
    /// `{"tool": "name", "input": {...}}` blocks.
    Json,
    /// `<shell>`, `<read_file path="...">`, `<write_file path="...">` style blocks.
    Xml,
}

/// How the model should be offered tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCalling {
    /// Send native function definitions.
    Native,
    /// Never send native tools; describe them in the prompt instead.
    Text(TextToolFormat),
    /// Use native tools when the server reports support, otherwise text.
    Auto(TextToolFormat),
}

/// Resolved decision for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Native,
    Text(TextToolFormat),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    pub temperature: f32,
    pub top_p: f32,
}

/// Sampling used for agent turns. Low temperature keeps small models from
/// wandering between tool calls; `top_p` trims the long tail.
pub const AGENT_SAMPLING: Sampling = Sampling {
    temperature: 0.2,
    top_p: 0.9,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelProfile {
    /// Stable identifier, shown in `/status` and logs.
    pub id: &'static str,
    pub tool_calling: ToolCalling,
    pub sampling: Sampling,
    /// Context window requested from the server (`num_ctx` for Ollama). The
    /// effective value is capped by what the model reports.
    pub context_tokens: u32,
    /// Constrain text-protocol replies with a JSON schema where the server
    /// supports it.
    pub structured_output: bool,
    /// Trained primarily on code; surfaced as a hint in the model picker.
    pub code_specialist: bool,
}

impl ModelProfile {
    /// Picks native or text tool calling.
    ///
    /// `server_supports_tools` is `None` when the server gave no capability
    /// information. `force_text` is the user's kill switch for misbehaving
    /// native tool support.
    pub fn resolve_tool_calling(
        &self,
        server_supports_tools: Option<bool>,
        force_text: bool,
    ) -> ToolMode {
        let fallback = match self.tool_calling {
            ToolCalling::Text(format) | ToolCalling::Auto(format) => format,
            ToolCalling::Native => TextToolFormat::Json,
        };
        if force_text {
            return ToolMode::Text(fallback);
        }
        match (self.tool_calling, server_supports_tools) {
            (ToolCalling::Text(format), _) => ToolMode::Text(format),
            // A server that explicitly says the model has no tool template
            // would reject or ignore native definitions.
            (ToolCalling::Native | ToolCalling::Auto(_), Some(false)) => ToolMode::Text(fallback),
            (ToolCalling::Native, _) => ToolMode::Native,
            (ToolCalling::Auto(_), Some(true)) => ToolMode::Native,
            (ToolCalling::Auto(format), None) => ToolMode::Text(format),
        }
    }
}

/// Returns the profile for a model name such as `qwen3.5:9b`,
/// `hf.co/org/Qwen3-14B-GGUF:Q4_K_M` or `deepseek-chat`.
pub fn profile_for_model(model: &str) -> ModelProfile {
    let name = normalize(model);
    let size = parameter_size_billions(&name);
    let moe = is_mixture_of_experts(&name);

    let local = |id, tool_calling, context_tokens| ModelProfile {
        id,
        tool_calling,
        sampling: AGENT_SAMPLING,
        context_tokens,
        structured_output: matches!(tool_calling, ToolCalling::Text(_) | ToolCalling::Auto(_)),
        code_specialist: name.contains("coder") || name.contains("code"),
    };

    if name.starts_with("deepseek-chat") || name.starts_with("deepseek-reasoner") {
        return ModelProfile {
            id: "deepseek-api",
            tool_calling: ToolCalling::Native,
            sampling: AGENT_SAMPLING,
            context_tokens: 131_072,
            structured_output: false,
            code_specialist: false,
        };
    }
    if name.contains("deepseek-coder") {
        return local(
            "deepseek-coder",
            ToolCalling::Text(TextToolFormat::Json),
            16_384,
        );
    }
    if name.contains("qwen") {
        if is_qwen35_9b(&name, size) {
            return local("qwen-9b", ToolCalling::Native, 16_384);
        }
        return match size {
            Some(b) if b <= 8.0 && !moe => {
                local("qwen-small", ToolCalling::Text(TextToolFormat::Json), 8_192)
            }
            // Mid-size, large and MoE builds handle native calls well. Unknown
            // sizes (community builds without a size tag) are treated as large.
            _ => local("qwen-large", ToolCalling::Native, 32_768),
        };
    }
    if name.starts_with("gpt-oss") {
        return local("gpt-oss", ToolCalling::Native, 32_768);
    }
    if is_hosted_openai_style(&name) {
        // Hosted Responses-API models not in the bundled catalog.
        return ModelProfile {
            id: "openai-hosted",
            tool_calling: ToolCalling::Native,
            sampling: AGENT_SAMPLING,
            context_tokens: 272_000,
            structured_output: false,
            code_specialist: false,
        };
    }
    const JSON_FAMILIES: [&str; 9] = [
        "llama",
        "hermes",
        "mistral",
        "mixtral",
        "gemma",
        "phi",
        "codellama",
        "starcoder",
        "granite",
    ];
    if JSON_FAMILIES.iter().any(|family| name.contains(family)) {
        return local("json-tools", ToolCalling::Auto(TextToolFormat::Json), 8_192);
    }
    local("generic", ToolCalling::Auto(TextToolFormat::Json), 8_192)
}

/// Lowercases and drops registry prefixes and quantization suffixes so
/// `hf.co/Qwen/Qwen3-14B-GGUF:Q4_K_M` and `qwen3:14b` look alike.
fn normalize(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    lower.rsplit('/').next().unwrap_or(&lower).to_string()
}

fn tokens(name: &str) -> impl Iterator<Item = &str> {
    name.split([':', '-', '_', '.', '/', ' '])
        .filter(|t| !t.is_empty())
}

/// Parses a parameter count such as `9b`, `14b` or `0.5b`. Tokens are matched
/// whole so `27b` is never read as `7b`.
fn parameter_size_billions(name: &str) -> Option<f32> {
    let parts: Vec<&str> = tokens(name).collect();
    let (index, value) = parts
        .iter()
        .enumerate()
        .find_map(|(i, token)| parse_size_token(token).map(|value| (i, value)))?;
    // Fractional sizes such as `1.5b` arrive split on the `.`; rejoin them.
    if let Some(prev) = index.checked_sub(1).and_then(|p| parts.get(p))
        && prev.chars().all(|c| c.is_ascii_digit())
        && name.contains(&format!("{prev}.{}", parts[index]))
        && let Ok(joined) = format!("{prev}.{}", parts[index].trim_end_matches('b')).parse()
    {
        return Some(joined);
    }
    Some(value)
}

fn parse_size_token(token: &str) -> Option<f32> {
    let digits = token.strip_suffix('b')?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<f32>().ok()
}

/// MoE tags carry the active parameter count, e.g. `35b-a3b`.
fn is_mixture_of_experts(name: &str) -> bool {
    tokens(name).any(|t| {
        t.len() > 2
            && t.starts_with('a')
            && t.ends_with('b')
            && t[1..t.len() - 1].chars().all(|c| c.is_ascii_digit())
    }) || name.contains("moe")
}

fn is_hosted_openai_style(name: &str) -> bool {
    name.starts_with("gpt-")
        || ["o1", "o3", "o4"]
            .iter()
            .any(|p| name == *p || name.starts_with(&format!("{p}-")))
}

fn is_qwen35_9b(name: &str, size: Option<f32>) -> bool {
    name.starts_with("qwen3.5") && matches!(size, None | Some(9.0))
}

#[cfg(test)]
mod tests;
