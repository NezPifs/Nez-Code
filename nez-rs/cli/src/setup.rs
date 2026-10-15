//! First-run setup: choose where model requests go.
//!
//! Runs once, before the interactive UI starts, when `config.toml` does not
//! pick a provider yet. It can also be run again with `nez setup`. API keys
//! go to the system credential store, never to `config.toml`.

use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;

use nez_core::config::Config;
use nez_core::config::edit::ConfigEdit;
use nez_core::config::edit::ConfigEditsBuilder;
use nez_model_profiles::TIERS;
use nez_model_provider_info::DEEPSEEK_API_KEY_ENV;
use nez_model_provider_info::DEEPSEEK_PROVIDER_ID;
use nez_model_provider_info::OLLAMA_OSS_PROVIDER_ID;

/// Written when setup finishes or is skipped, so it is not offered again.
pub const SETUP_MARKER: &str = ".setup-complete";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupChoice {
    Ollama { model: String },
    DeepSeek { api_key: String },
    Skip,
}

/// What setup found on this machine.
#[derive(Debug, Clone, Default)]
pub struct SetupProbe {
    pub ollama_running: bool,
    pub installed_models: Vec<String>,
    pub gpu_memory_mib: Option<u64>,
    pub gpu_memory_source: Option<&'static str>,
    pub deepseek_key_in_env: bool,
}

/// Terminal interaction, abstracted for tests.
pub trait Prompter {
    fn say(&mut self, text: &str);
    fn ask(&mut self, prompt: &str) -> io::Result<String>;
    /// Reads without echoing.
    fn ask_secret(&mut self, prompt: &str) -> io::Result<String>;
}

/// Whether to offer setup before the interactive UI starts.
pub fn needs_first_run_setup(nez_home: &Path) -> bool {
    if nez_home.join(SETUP_MARKER).exists() {
        return false;
    }
    match std::fs::read_to_string(nez_home.join("config.toml")) {
        Ok(contents) => !configures_provider(&contents),
        Err(_) => true,
    }
}

fn configures_provider(contents: &str) -> bool {
    contents
        .parse::<toml::Table>()
        .map(|table| table.contains_key("model_provider"))
        // An unreadable config is reported by the normal config loader.
        .unwrap_or(true)
}

pub fn choose(probe: &SetupProbe, prompter: &mut dyn Prompter) -> io::Result<SetupChoice> {
    let recommended = nez_model_profiles::recommended_model_for_vram(probe.gpu_memory_mib);
    prompter.say("Welcome to Nez-Code. Where should model requests go?\n");
    let ollama_state = if probe.ollama_running {
        "Ollama is running"
    } else {
        "Ollama is not running yet"
    };
    prompter.say(&format!(
        "  1) Local models with Ollama: private, runs on this computer ({ollama_state})"
    ));
    prompter.say("  2) DeepSeek API: hosted, needs an API key");
    prompter.say("  3) Skip for now: keep the defaults (local Ollama)\n");
    let choice = loop {
        match prompter.ask("Choose 1, 2 or 3 [1]: ")?.trim() {
            "" | "1" => break 1,
            "2" => break 2,
            "3" => break 3,
            other => prompter.say(&format!("`{other}` is not one of the options.")),
        }
    };
    match choice {
        1 => {
            match (probe.gpu_memory_mib, probe.gpu_memory_source) {
                (Some(mib), Some(source)) => prompter.say(&format!(
                    "\nDetected {:.1} GB of GPU memory ({source}).",
                    mib as f64 / 1024.0
                )),
                _ => prompter.say("\nCould not detect GPU memory."),
            }
            prompter.say("Models by GPU memory:");
            for tier in TIERS {
                let marker = if tier.model == recommended.model {
                    "  (recommended)"
                } else {
                    ""
                };
                let installed = if probe.installed_models.iter().any(|m| m == tier.model) {
                    ", installed"
                } else {
                    ""
                };
                prompter.say(&format!(
                    "  {}: {}{installed}{marker}",
                    tier.model, tier.summary
                ));
            }
            let answer = prompter.ask(&format!("Model [{}]: ", recommended.model))?;
            let model = match answer.trim() {
                "" => recommended.model.to_string(),
                other => other.to_string(),
            };
            Ok(SetupChoice::Ollama { model })
        }
        2 => {
            if probe.deepseek_key_in_env {
                prompter.say(&format!(
                    "\n{DEEPSEEK_API_KEY_ENV} is set in your environment; Nez-Code will use it."
                ));
                return Ok(SetupChoice::DeepSeek {
                    api_key: String::new(),
                });
            }
            prompter.say(
                "\nCreate a key at https://platform.deepseek.com/api_keys. It is stored in your \
                 system credential store, not in a file.",
            );
            loop {
                let key = prompter.ask_secret("DeepSeek API key (empty to cancel): ")?;
                let key = key.trim();
                if key.is_empty() {
                    prompter.say("No key entered; skipping setup.");
                    return Ok(SetupChoice::Skip);
                }
                if key.chars().any(char::is_whitespace) {
                    prompter.say("That does not look like an API key; it contains spaces.");
                    continue;
                }
                return Ok(SetupChoice::DeepSeek {
                    api_key: key.to_string(),
                });
            }
        }
        _ => Ok(SetupChoice::Skip),
    }
}

/// Persists a choice. `save_key` stores an API key in the credential store.
pub fn apply(
    nez_home: &Path,
    choice: &SetupChoice,
    save_key: impl FnOnce(&str, &str) -> Result<(), String>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(nez_home)?;
    let (provider, model) = match choice {
        SetupChoice::Ollama { model } => (Some(OLLAMA_OSS_PROVIDER_ID), Some(model.as_str())),
        SetupChoice::DeepSeek { api_key } => {
            if !api_key.is_empty() {
                save_key(DEEPSEEK_API_KEY_ENV, api_key).map_err(anyhow::Error::msg)?;
            }
            (Some(DEEPSEEK_PROVIDER_ID), Some("deepseek-chat"))
        }
        SetupChoice::Skip => (None, None),
    };
    if let Some(provider) = provider {
        ConfigEditsBuilder::new(nez_home)
            .set_model(model, /*effort*/ None)
            .with_edits([ConfigEdit::SetPath {
                segments: vec!["model_provider".to_string()],
                value: toml_edit::value(provider),
            }])
            .apply_blocking()?;
    }
    std::fs::write(nez_home.join(SETUP_MARKER), "")?;
    Ok(())
}

async fn probe(config: &Config) -> SetupProbe {
    let (_, memory) = nez_ollama::hardware::recommended_tier();
    let installed = match nez_ollama::OllamaClient::try_from_oss_provider(config).await {
        Ok(client) => client.fetch_models().await.ok(),
        Err(_) => None,
    };
    SetupProbe {
        ollama_running: installed.is_some(),
        installed_models: installed.unwrap_or_default(),
        gpu_memory_mib: memory.as_ref().map(|m| m.mib),
        gpu_memory_source: memory.as_ref().map(|m| m.source),
        deepseek_key_in_env: std::env::var(DEEPSEEK_API_KEY_ENV)
            .is_ok_and(|v| !v.trim().is_empty()),
    }
}

/// Runs setup in the terminal. Returns without changes when not attached to
/// one.
pub async fn run_interactive(config: &Config) -> anyhow::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("`nez setup` needs an interactive terminal");
    }
    let probe = probe(config).await;
    let mut prompter = TerminalPrompter;
    let choice = choose(&probe, &mut prompter)?;
    apply(&config.nez_home, &choice, |env_key, key| {
        nez_model_provider_info::provider_keys::save_provider_key(env_key, key)
    })?;
    match &choice {
        SetupChoice::Ollama { model } => {
            if !probe.ollama_running {
                println!(
                    "\nStart Ollama (https://ollama.com/download), then run `ollama pull {model}`."
                );
            } else if !probe.installed_models.iter().any(|m| m == model) {
                let answer = prompter.ask(&format!("Download {model} now? [Y/n]: "))?;
                if matches!(answer.trim(), "" | "y" | "Y" | "yes") {
                    let client = nez_ollama::OllamaClient::try_from_oss_provider(config).await?;
                    let mut reporter = nez_ollama::CliProgressReporter::new();
                    client.pull_with_reporter(model, &mut reporter).await?;
                }
            }
            println!("\nUsing {model} on local Ollama. Change it any time with /model.");
        }
        SetupChoice::DeepSeek { .. } => {
            println!("\nUsing deepseek-chat on the DeepSeek API.");
        }
        SetupChoice::Skip => {
            println!("\nSkipped. Run `nez setup` whenever you want to choose a provider.");
        }
    }
    Ok(())
}

struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn say(&mut self, text: &str) {
        println!("{text}");
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        print!("{prompt}");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        Ok(line)
    }

    fn ask_secret(&mut self, prompt: &str) -> io::Result<String> {
        use crossterm::event::Event;
        use crossterm::event::KeyCode;
        use crossterm::event::KeyEventKind;
        use crossterm::event::KeyModifiers;

        print!("{prompt}");
        io::stdout().flush()?;
        crossterm::terminal::enable_raw_mode()?;
        let mut secret = String::new();
        let result = loop {
            match crossterm::event::read() {
                Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Enter => break Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                    }
                    KeyCode::Char(c) => secret.push(c),
                    KeyCode::Backspace => {
                        secret.pop();
                    }
                    _ => {}
                },
                // Pasted keys arrive as one event in terminals with bracketed paste.
                Ok(Event::Paste(text)) => secret.push_str(&text),
                Ok(_) => {}
                Err(err) => break Err(err),
            }
        };
        crossterm::terminal::disable_raw_mode()?;
        println!();
        result.map(|()| secret)
    }
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
