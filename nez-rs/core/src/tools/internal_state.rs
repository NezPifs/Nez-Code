//! Keeps model tool calls away from Nez-Code's own state under `NEZ_HOME`.
//!
//! Session rollouts, credentials and state databases are written by Nez-Code
//! while it runs. A model that edits or deletes them corrupts the session it
//! is running in, so shell commands and patches that name these locations are
//! refused before approval. User-editable files in `NEZ_HOME` (`config.toml`,
//! `AGENTS.md`, skills, rules, memories) stay reachable.

use std::path::Component;
use std::path::Path;

/// Entries directly under `NEZ_HOME` that only Nez-Code itself writes.
const PROTECTED_ENTRIES: &[&str] = &[
    "sessions",
    "archived_sessions",
    "auth.json",
    "history.jsonl",
    "secrets",
    "log",
    "logs",
    "shell_snapshots",
    "models_cache.json",
    ".sandbox",
    ".tmp",
    "tmp",
];

fn is_protected_entry(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    PROTECTED_ENTRIES.contains(&name.as_str())
        || name.ends_with(".sqlite")
        || name.ends_with(".sqlite-wal")
        || name.ends_with(".sqlite-shm")
}

pub(crate) fn refusal_message(location: &str) -> String {
    format!(
        "Refusing to touch `{location}`: it is Nez-Code's internal session state and changing it \
         would corrupt the running session. Work on the user's files instead."
    )
}

/// Whether `path` is, or lies inside, a protected entry of `nez_home`.
pub(crate) fn is_internal_state_path(path: &Path, nez_home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(nez_home) else {
        return false;
    };
    match relative.components().next() {
        Some(Component::Normal(first)) => is_protected_entry(&first.to_string_lossy()),
        _ => false,
    }
}

/// Finds a reference to protected state in a shell command. Returns the
/// matched location for the refusal message.
pub(crate) fn find_internal_state_reference(command: &str, nez_home: &Path) -> Option<String> {
    let normalized = command.replace('\\', "/").to_ascii_lowercase();
    let home = nez_home
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let mut prefixes = vec![
        home,
        "$nez_home".to_string(),
        "${nez_home}".to_string(),
        "%nez_home%".to_string(),
        "$env:nez_home".to_string(),
    ];
    // Shell spellings of the default location.
    if nez_home.file_name().is_some_and(|name| name == ".nez") {
        prefixes.extend(
            [
                "~/.nez",
                "$home/.nez",
                "${home}/.nez",
                "%userprofile%/.nez",
                "$env:userprofile/.nez",
                "${env:userprofile}/.nez",
            ]
            .map(str::to_string),
        );
    }
    for prefix in prefixes.iter().filter(|p| !p.is_empty()) {
        let needle = format!("{prefix}/");
        let mut search = 0;
        while let Some(found) = normalized[search..].find(&needle) {
            let start = search + found + needle.len();
            let entry: String = normalized[start..]
                .chars()
                .take_while(|c| !matches!(c, '/' | ' ' | '\'' | '"' | ';' | '|' | '&' | ')' | '`'))
                .collect();
            if is_protected_entry(&entry) {
                return Some(format!("{prefix}/{entry}"));
            }
            search = start;
        }
    }
    None
}

#[cfg(test)]
#[path = "internal_state_tests.rs"]
mod tests;
