//! Derives the enterprise credential owner from the active Nez account.

use nez_config::McpEmaAuthScope;
use nez_login::NezAuth;

pub fn ema_auth_scope(auth: Option<&NezAuth>) -> Option<McpEmaAuthScope> {
    let auth = auth?;
    McpEmaAuthScope::new(auth.get_chatgpt_user_id()?, auth.get_account_id()?)
}
