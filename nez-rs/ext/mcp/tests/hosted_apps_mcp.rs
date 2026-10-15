use std::sync::Arc;

use nez_config::McpServerTransportConfig;
use nez_core::McpManager;
use nez_core::config::Config;
use nez_core::config::ConfigBuilder;
use nez_core::plugins_manager_for_config;
use nez_extension_api::ExtensionRegistryBuilder;
use nez_extension_api::McpServerContribution;
use nez_extension_api::McpServerContributionContext;
use nez_extension_api::McpServerContributor;
use nez_login::AuthManager;
use nez_login::NezAuth;
use nez_login::test_support::auth_manager_from_optional_auth;
use nez_mcp::NEZ_APPS_MCP_SERVER_NAME;
use pretty_assertions::assert_eq;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn contributes_hosted_plugin_runtime_without_an_executor() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            ("chatgpt_base_url".to_string(), "https://chatgpt.com".into()),
        ])
        .build()
        .await?;
    let auth = NezAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(NEZ_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should be contributed as a configured server")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("hosted plugin runtime should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn runtime_overlay_preserves_disabled_server() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.nez_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
            ("mcp_servers.nez_apps.enabled".to_string(), false.into()),
        ])
        .build()
        .await?;
    let auth = NezAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(NEZ_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should remain configured")?;

    assert!(!server.enabled());
    Ok(())
}

#[tokio::test]
async fn default_fallback_overwrites_reserved_config_without_an_extension() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.nez_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let auth = NezAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = McpManager::new(Arc::new(plugins_manager_for_config(
        &config,
        AuthManager::from_auth_for_testing(auth.clone()),
    )));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(NEZ_APPS_MCP_SERVER_NAME)
        .ok_or("default Apps MCP should be present")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("default Apps MCP should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn later_extension_can_remove_same_name_registration() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = NezAuth::create_dummy_chatgpt_auth_for_testing();
    let mut builder = ExtensionRegistryBuilder::new();
    nez_mcp_extension::install(&mut builder);
    builder.mcp_server_contributor(Arc::new(RemoveNezApps));
    let manager = McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            &config,
            AuthManager::from_auth_for_testing(auth.clone()),
        )),
        Arc::new(builder.build()),
        nez_core::NezAppsToolsCache::default(),
    );

    let servers = manager.effective_servers(&config, Some(&auth)).await;

    assert!(!servers.contains_key(NEZ_APPS_MCP_SERVER_NAME));
    Ok(())
}

#[tokio::test]
async fn hosted_apps_mcp_requires_chatgpt_auth() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = NezAuth::from_api_key("test");
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    assert!(!servers.contains_key(NEZ_APPS_MCP_SERVER_NAME));

    Ok(())
}

#[tokio::test]
async fn disabled_apps_remove_reserved_server_config_for_all_hosts() -> TestResult {
    let nez_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .nez_home(nez_home.path().to_path_buf())
        .fallback_cwd(Some(nez_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), false.into()),
            (
                "mcp_servers.nez_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let managers = [
        installed_manager(&config, /*auth*/ None),
        McpManager::new(Arc::new(plugins_manager_for_config(
            &config,
            auth_manager_from_optional_auth(/*auth*/ None),
        ))),
    ];
    for manager in managers {
        let servers = manager.runtime_servers(&config).await;
        assert!(!servers.contains_key(NEZ_APPS_MCP_SERVER_NAME));
    }
    Ok(())
}

fn installed_manager(config: &Config, auth: Option<NezAuth>) -> McpManager {
    let mut builder = ExtensionRegistryBuilder::new();
    nez_mcp_extension::install(&mut builder);
    McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            config,
            auth_manager_from_optional_auth(auth),
        )),
        Arc::new(builder.build()),
        nez_core::NezAppsToolsCache::default(),
    )
}

struct RemoveNezApps;

impl McpServerContributor<Config> for RemoveNezApps {
    fn id(&self) -> &'static str {
        "remove_nez_apps"
    }

    fn contribute<'a>(
        &'a self,
        _context: McpServerContributionContext<'a, Config>,
    ) -> nez_extension_api::ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            vec![McpServerContribution::Remove {
                name: NEZ_APPS_MCP_SERVER_NAME.to_string(),
            }]
        })
    }
}
