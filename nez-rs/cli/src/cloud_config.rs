use anyhow::Context;
use anyhow::Result;
use nez_app_server::in_process::EmbeddedNetworkPolicy;
use nez_cloud_config::cloud_config_bundle_loader_for_storage;
use nez_config::CloudConfigBundleLoader;
use nez_config::ConfigLoadOptions;
use nez_core::config::Config;
use nez_core::config::ConfigBuilder;
use nez_core::config::ConfigOverrides;
use nez_core::config::LoaderOverrides;
use nez_core::config::bootstrap_auth_config;
use nez_core::config::find_nez_home;
use nez_core::config::load_config_toml_with_layer_stack;
use nez_utils_absolute_path::AbsolutePathBuf;
use nez_utils_cli::CliConfigOverrides;

pub(crate) async fn load_config(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<Config> {
    let mut config = config_builder(
        config_overrides,
        loader_overrides,
        ConfigOverrides::default(),
    )
    .await?
    .build()
    .await
    .context("failed to load configuration")?;
    EmbeddedNetworkPolicy::default().activate(&mut config);
    Ok(config)
}

pub(crate) async fn config_builder(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
    harness_overrides: ConfigOverrides,
) -> Result<ConfigBuilder> {
    let network_policy = EmbeddedNetworkPolicy::load(&loader_overrides).await;
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let nez_home = find_nez_home().context("failed to resolve NEZ_HOME")?;
    let cwd = match harness_overrides.cwd.as_deref() {
        Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd),
        None => AbsolutePathBuf::current_dir(),
    }
    .context("failed to resolve current directory")?;
    let bootstrap_config = load_config_toml_with_layer_stack(
        nez_home.as_path(),
        Some(&cwd),
        cli_overrides.clone(),
        ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await
    .context("failed to load bootstrap configuration")?;
    let cloud_config_bundle = cloud_config_bundle_loader_for_storage(
        network_policy.bind_bootstrap_auth(
            bootstrap_auth_config(nez_home.as_path(), &bootstrap_config)
                .context("failed to resolve cloud configuration authentication")?,
        ),
        /*enable_nez_api_key_env*/ false,
    )
    .await
    .context("failed to initialize cloud configuration authentication")?;

    Ok(ConfigBuilder::default()
        .nez_home(nez_home.to_path_buf())
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .harness_overrides(harness_overrides)
        .cloud_config_bundle(cloud_config_bundle)
        .fallback_cwd(Some(cwd.to_path_buf())))
}
