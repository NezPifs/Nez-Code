use super::*;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;

#[derive(Default)]
struct Scripted {
    answers: VecDeque<String>,
    secrets: VecDeque<String>,
    output: Vec<String>,
}

impl Scripted {
    fn new(answers: &[&str], secrets: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|s| format!("{s}\n")).collect(),
            secrets: secrets.iter().map(ToString::to_string).collect(),
            output: Vec::new(),
        }
    }
}

impl Prompter for Scripted {
    fn say(&mut self, text: &str) {
        self.output.push(text.to_string());
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        self.output.push(prompt.to_string());
        Ok(self.answers.pop_front().unwrap_or_default())
    }

    fn ask_secret(&mut self, prompt: &str) -> io::Result<String> {
        self.output.push(prompt.to_string());
        Ok(self.secrets.pop_front().unwrap_or_default())
    }
}

fn probe_with_gpu(mib: Option<u64>) -> SetupProbe {
    SetupProbe {
        ollama_running: true,
        installed_models: vec!["qwen3.5:9b".to_string()],
        gpu_memory_mib: mib,
        gpu_memory_source: mib.map(|_| "nvidia-smi"),
        deepseek_key_in_env: false,
    }
}

#[test]
fn ollama_is_the_default_and_the_gpu_tier_is_recommended() {
    let mut prompter = Scripted::new(&["", ""], &[]);
    let choice = choose(&probe_with_gpu(Some(16 * 1024)), &mut prompter).expect("choose");
    assert_eq!(
        choice,
        SetupChoice::Ollama {
            model: "deepseek-coder-v2:16b".to_string()
        }
    );
    let transcript = prompter.output.join("\n");
    assert!(
        transcript.contains("Detected 16.0 GB of GPU memory (nvidia-smi)"),
        "{transcript}"
    );
    assert!(
        transcript
            .contains("qwen3.5:9b: 8 GB+ VRAM: Qwen 3.5 9B with native tool calls, installed")
    );
}

#[test]
fn a_typed_model_overrides_the_recommendation() {
    let mut prompter = Scripted::new(&["1", "llama3.1:8b"], &[]);
    assert_eq!(
        choose(&probe_with_gpu(None), &mut prompter).expect("choose"),
        SetupChoice::Ollama {
            model: "llama3.1:8b".to_string()
        }
    );
}

#[test]
fn invalid_choices_are_asked_again() {
    let mut prompter = Scripted::new(&["7", "3"], &[]);
    assert_eq!(
        choose(&probe_with_gpu(None), &mut prompter).expect("choose"),
        SetupChoice::Skip
    );
    assert!(
        prompter
            .output
            .iter()
            .any(|l| l.contains("`7` is not one of the options"))
    );
}

#[test]
fn deepseek_reads_the_key_without_echo() {
    let mut prompter = Scripted::new(&["2"], &["sk with spaces", "sk-abc123"]);
    assert_eq!(
        choose(&probe_with_gpu(None), &mut prompter).expect("choose"),
        SetupChoice::DeepSeek {
            api_key: "sk-abc123".to_string()
        }
    );
    let mut cancelled = Scripted::new(&["2"], &[""]);
    assert_eq!(
        choose(&probe_with_gpu(None), &mut cancelled).expect("choose"),
        SetupChoice::Skip
    );
}

#[test]
fn deepseek_uses_an_existing_environment_key() {
    let mut probe = probe_with_gpu(None);
    probe.deepseek_key_in_env = true;
    let mut prompter = Scripted::new(&["2"], &[]);
    assert_eq!(
        choose(&probe, &mut prompter).expect("choose"),
        SetupChoice::DeepSeek {
            api_key: String::new()
        }
    );
}

#[test]
fn applying_deepseek_stores_the_key_outside_the_config() {
    let home = tempfile::tempdir().expect("tempdir");
    let mut saved = None;
    apply(
        home.path(),
        &SetupChoice::DeepSeek {
            api_key: "sk-secret".to_string(),
        },
        |env_key, key| {
            saved = Some((env_key.to_string(), key.to_string()));
            Ok(())
        },
    )
    .expect("apply");
    assert_eq!(
        saved,
        Some(("DEEPSEEK_API_KEY".to_string(), "sk-secret".to_string()))
    );
    let config = std::fs::read_to_string(home.path().join("config.toml")).expect("config");
    assert!(config.contains("model_provider = \"deepseek\""), "{config}");
    assert!(config.contains("model = \"deepseek-chat\""), "{config}");
    assert!(!config.contains("sk-secret"));
    assert!(!needs_first_run_setup(home.path()));
}

#[test]
fn a_failed_key_save_writes_nothing() {
    let home = tempfile::tempdir().expect("tempdir");
    let result = apply(
        home.path(),
        &SetupChoice::DeepSeek {
            api_key: "sk".to_string(),
        },
        |_, _| Err("no credential store".to_string()),
    );
    assert!(result.is_err());
    assert!(!home.path().join("config.toml").exists());
    assert!(needs_first_run_setup(home.path()));
}

#[test]
fn ollama_and_skip_are_remembered() {
    let home = tempfile::tempdir().expect("tempdir");
    assert!(needs_first_run_setup(home.path()));
    apply(
        home.path(),
        &SetupChoice::Ollama {
            model: "qwen3.5:9b".to_string(),
        },
        |_, _| unreachable!("no key for Ollama"),
    )
    .expect("apply");
    let config = std::fs::read_to_string(home.path().join("config.toml")).expect("config");
    assert!(config.contains("model_provider = \"ollama\""), "{config}");
    assert!(config.contains("model = \"qwen3.5:9b\""), "{config}");

    let skipped = tempfile::tempdir().expect("tempdir");
    apply(skipped.path(), &SetupChoice::Skip, |_, _| unreachable!()).expect("apply");
    assert!(!skipped.path().join("config.toml").exists());
    assert!(!needs_first_run_setup(skipped.path()));
}

#[test]
fn existing_provider_configuration_skips_setup() {
    let home = tempfile::tempdir().expect("tempdir");
    std::fs::write(home.path().join("config.toml"), "model = \"x\"\n").expect("write");
    assert!(needs_first_run_setup(home.path()));
    std::fs::write(
        home.path().join("config.toml"),
        "model_provider = \"lmstudio\"\n",
    )
    .expect("write");
    assert!(!needs_first_run_setup(home.path()));
}
