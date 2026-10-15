/// Update action the CLI should perform after the TUI exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Replace the local daemon after restoring the terminal.
    Daemon(DaemonUpdateSource),
    /// Replace this binary with the latest GitHub release (`nez update --yes`).
    SelfUpdate,
}

impl UpdateAction {
    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::Daemon(source) => ("nez", source.command_args()),
            UpdateAction::SelfUpdate => ("nez", &["update", "--yes"]),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

/// Release builds always update themselves from GitHub releases; the
/// installers put `nez` somewhere the current user can write.
#[cfg(not(debug_assertions))]
pub fn get_update_action() -> Option<UpdateAction> {
    Some(UpdateAction::SelfUpdate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn self_update_reruns_this_binary() {
        assert_eq!(UpdateAction::SelfUpdate.command_str(), "nez update --yes");
        assert_eq!(
            UpdateAction::Daemon(DaemonUpdateSource::ThisCli).command_str(),
            "nez app-server daemon update --from-cli --yes"
        );
    }
}

/// Package source explicitly selected by the user in the daemon menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonUpdateSource {
    PublicStable,
    ThisCli,
}

impl DaemonUpdateSource {
    pub fn command_args(self) -> &'static [&'static str] {
        match self {
            Self::PublicStable => &["app-server", "daemon", "update"],
            Self::ThisCli => &["app-server", "daemon", "update", "--from-cli", "--yes"],
        }
    }
}
