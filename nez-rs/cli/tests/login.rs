use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use nez_config::types::AuthCredentialsStoreMode;
use nez_login::AuthKeyringBackendKind;
use nez_login::NEZ_ACCESS_TOKEN_ENV_VAR;
use nez_login::REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR;
use nez_login::login_with_bedrock_access_keys;
use nez_protocol::shell_environment::OPENAI_FEDERATION_RULE_ID_ENV_VAR;
use nez_protocol::shell_environment::OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

fn nez_command(nez_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(nez_utils_cargo_bin::cargo_bin("nez")?);
    cmd.env("NEZ_HOME", nez_home);
    Ok(cmd)
}

fn write_file_auth_config(nez_home: &Path) -> Result<()> {
    std::fs::write(
        nez_home.join("config.toml"),
        "cli_auth_credentials_store = \"file\"\n",
    )?;
    Ok(())
}

fn read_auth_json(nez_home: &Path) -> Result<Value> {
    let auth_json = std::fs::read_to_string(nez_home.join("auth.json"))?;
    Ok(serde_json::from_str(&auth_json)?)
}

#[test]
fn login_with_api_key_reads_stdin_and_writes_auth_json() -> Result<()> {
    let nez_home = TempDir::new()?;
    write_file_auth_config(nez_home.path())?;

    let mut cmd = nez_command(nez_home.path())?;
    cmd.args([
        "-c",
        "forced_login_method=\"api\"",
        "login",
        "--with-api-key",
    ])
    .write_stdin("sk-test\n")
    .assert()
    .success()
    .stderr(contains("Successfully logged in"));

    let auth = read_auth_json(nez_home.path())?;
    assert_eq!(auth["OPENAI_API_KEY"], "sk-test");
    assert!(auth.get("tokens").is_none());
    assert!(auth.get("agent_identity").is_none());

    Ok(())
}

#[test]
fn login_status_reports_auth_storage_errors() -> Result<()> {
    let nez_home = TempDir::new()?;
    write_file_auth_config(nez_home.path())?;
    std::fs::write(nez_home.path().join("auth.json"), "{invalid json")?;

    nez_command(nez_home.path())?
        .args(["login", "status"])
        .assert()
        .failure()
        .stderr(contains("Error checking login status:"));

    Ok(())
}

#[test]
fn login_status_validates_configured_workload_identity() -> Result<()> {
    let nez_home = TempDir::new()?;
    write_file_auth_config(nez_home.path())?;
    let missing_assertion = nez_home.path().join("missing-identity-token");

    nez_command(nez_home.path())?
        .env_remove(NEZ_ACCESS_TOKEN_ENV_VAR)
        .env(OPENAI_FEDERATION_RULE_ID_ENV_VAR, "rule-test")
        .env(OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR, &missing_assertion)
        .args(["login", "status"])
        .assert()
        .failure()
        .stderr(contains("could not read workload identity assertion file"));

    Ok(())
}

#[test]
fn logout_clears_only_the_selected_bedrock_provider() -> Result<()> {
    for (model_provider_id, managed_bedrock_auth, model) in [
        ("amazon-bedrock", true, "openai.gpt-5.6-sol"),
        ("amazon-bedrock-runtime", true, "global.openai.gpt-5.6-sol"),
        ("openai", true, "gpt-5.6-sol"),
        ("amazon-bedrock", false, "gpt-5.6-sol"),
        ("amazon-bedrock-runtime", false, "us.openai.gpt-5.6-sol"),
        ("openai", false, "gpt-5.6-sol"),
    ] {
        let nez_home = TempDir::new()?;
        let config_path = nez_home.path().join("config.toml");
        std::fs::write(
            &config_path,
            format!(
                "cli_auth_credentials_store = \"file\"\n\
                 model_provider = \"{model_provider_id}\"\n\
                 model = \"{model}\"\n\
                 model_reasoning_effort = \"high\"\n\
                 [model_providers.amazon-bedrock]\n\
                 base_url = \"https://mantle.example.com/v1\"\n\
                 [model_providers.amazon-bedrock.aws]\n\
                 profile = \"mantle-profile\"\n\
                 region = \"us-west-2\"\n\
                 auth_refresh = {{ command = \"aws\", args = [\"sso\", \"login\"] }}\n\
                 [model_providers.amazon-bedrock-runtime]\n\
                 base_url = \"https://runtime.example.com/v1\"\n\
                 [model_providers.amazon-bedrock-runtime.aws]\n\
                 profile = \"runtime-profile\"\n\
                 region = \"us-east-1\"\n\
                 auth_refresh = {{ command = \"aws\", args = [\"login\"] }}\n"
            ),
        )?;
        if managed_bedrock_auth {
            login_with_bedrock_access_keys(
                nez_home.path(),
                "managed-access-key-id",
                "managed-secret-access-key",
                Some("managed-session-token"),
                AuthCredentialsStoreMode::File,
                AuthKeyringBackendKind::default(),
            )?;
        }
        let mut expected_config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path)?)?;
        if model_provider_id != "openai" {
            let expected_root = expected_config
                .as_table_mut()
                .expect("config should be a table");
            expected_root.remove("model_provider");
            expected_root.remove("model");
            expected_root["model_providers"][model_provider_id]
                .as_table_mut()
                .expect("selected Bedrock provider should be a table")
                .remove("aws");
        }
        let expected_message = if managed_bedrock_auth || model_provider_id != "openai" {
            "Successfully logged out"
        } else {
            "Not logged in"
        };

        nez_command(nez_home.path())?
            .env_remove(NEZ_ACCESS_TOKEN_ENV_VAR)
            .env("AWS_ACCESS_KEY_ID", "environment-access-key-id")
            .env("AWS_SECRET_ACCESS_KEY", "environment-secret-access-key")
            .args(["logout"])
            .assert()
            .success()
            .stderr(contains(expected_message));

        assert!(!nez_home.path().join("auth.json").exists());
        let actual_config: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
        assert_eq!(actual_config, expected_config);
    }

    Ok(())
}

#[tokio::test]
async fn logout_survives_enterprise_cleanup_failure_with_xaa_disabled() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(path("/backend-api/wham/config/bundle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "config_toml": {"enterprise_managed": []},
            "requirements_toml": {"enterprise_managed": []},
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "cli_auth_credentials_store = \"file\"\nchatgpt_base_url = \"{}/backend-api\"\n[features]\nuse_xaa = false\n[mcp_enterprise_managed_auth.idp]\nissuer = \"https://idp.example\"\nclient_id = \"enterprise-client\"\n[mcp_servers.enterprise]\nurl = \"https://resource.example/mcp\"\nauth = \"ema_auth\"\nbearer_token_env_var = \"UNUSED_TOKEN\"\n",
            server.uri()
        ),
    )?;
    write_chatgpt_auth(
        home.path(),
        ChatGptAuthFixture::new("account-access")
            .account_id("workspace")
            .chatgpt_user_id("user"),
        AuthCredentialsStoreMode::File,
    )?;
    // Fail before touching the real keyring; the subprocess owns this isolated home.
    std::fs::write(home.path().join("mcp-oauth-locks"), "not a directory")?;
    for enabled in [false, true] {
        nez_command(home.path())?
            .current_dir(home.path())
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("NEZ_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove(NEZ_ACCESS_TOKEN_ENV_VAR)
            .args([
                "-c",
                &format!("features.use_xaa={enabled}"),
                "mcp",
                "logout",
                "enterprise",
            ])
            .assert()
            .failure()
            .stderr(contains("failed to delete enterprise authorization"));
        assert!(home.path().join("auth.json").exists());
    }
    nez_command(home.path())?
        .current_dir(home.path())
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .env(
            REVOKE_TOKEN_URL_OVERRIDE_ENV_VAR,
            format!("{}/oauth/revoke", server.uri()),
        )
        .env_remove("NEZ_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove(NEZ_ACCESS_TOKEN_ENV_VAR)
        .args(["logout"])
        .assert()
        .success()
        .stderr(contains("continuing account logout"))
        .stderr(contains("Successfully logged out"));
    assert!(!home.path().join("auth.json").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn debug_prompt_input_follows_authenticated_attribution_setting() -> Result<()> {
    let server = MockServer::start().await;
    let request_count = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/settings/user"))
        .and(header("chatgpt-account-id", "workspace-123"))
        .respond_with(move |_request: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(json!({
                "commit_attribution_enabled": request_count.fetch_add(1, Ordering::SeqCst) == 0,
            }))
        })
        .expect(2)
        .mount(&server)
        .await;
    let nez_home = TempDir::new()?;
    std::fs::write(
        nez_home.path().join("config.toml"),
        format!(
            "cli_auth_credentials_store = \"file\"\nchatgpt_base_url = \"{}/backend-api\"\n",
            server.uri()
        ),
    )?;
    write_chatgpt_auth(
        nez_home.path(),
        ChatGptAuthFixture::new("chatgpt-token")
            .account_id("workspace-123")
            .plan_type("enterprise"),
        AuthCredentialsStoreMode::File,
    )?;
    for enabled in [true, false] {
        let output = nez_command(nez_home.path())?
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("NEZ_ACCESS_TOKEN")
            .env_remove("OPENAI_API_KEY")
            .args(["debug", "prompt-input"])
            .output()?;
        assert!(output.status.success());
        let prompt = String::from_utf8(output.stdout)?;
        assert_eq!(
            prompt.contains("Co-authored-by: Nez <noreply@openai.com>"),
            enabled
        );
        assert!(!prompt.contains("attribution is disabled for the current workspace"));
    }
    server.verify().await;
    Ok(())
}

