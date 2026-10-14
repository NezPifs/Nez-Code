use std::sync::Arc;
use std::sync::Weak;

use nez_core::ThreadManager;
use nez_core::config::Config;
use nez_extension_api::ExtensionRegistryBuilder;
use nez_login::AuthManager;

mod async_scorer;
mod sync_reviewer;

pub use sync_reviewer::install as install_reviewer;

/// Installs the guardian contributors into the extension registry.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    auth_manager: Arc<AuthManager>,
    thread_manager: Weak<ThreadManager>,
) {
    async_scorer::install(registry, auth_manager, thread_manager.clone());
    install_reviewer(registry, thread_manager);
}
