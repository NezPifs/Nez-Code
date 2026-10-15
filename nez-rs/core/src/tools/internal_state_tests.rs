use super::*;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn home() -> PathBuf {
    PathBuf::from("/home/dev/.nez")
}

#[test]
fn session_state_paths_are_protected() {
    for path in [
        "/home/dev/.nez/sessions",
        "/home/dev/.nez/sessions/2026/10/15/rollout.jsonl",
        "/home/dev/.nez/auth.json",
        "/home/dev/.nez/state_5.sqlite",
        "/home/dev/.nez/.tmp/plugins",
    ] {
        assert!(is_internal_state_path(Path::new(path), &home()), "{path}");
    }
}

#[test]
fn user_editable_files_and_other_paths_are_allowed() {
    for path in [
        "/home/dev/.nez/config.toml",
        "/home/dev/.nez/AGENTS.md",
        "/home/dev/.nez/skills/review/SKILL.md",
        "/home/dev/.nez",
        "/home/dev/project/sessions/notes.md",
    ] {
        assert!(!is_internal_state_path(Path::new(path), &home()), "{path}");
    }
}

#[test]
fn commands_naming_protected_state_are_found() {
    let cases = [
        ("rm -rf /home/dev/.nez/sessions", "/home/dev/.nez/sessions"),
        ("cat ~/.nez/auth.json", "~/.nez/auth.json"),
        (
            r"Remove-Item $env:USERPROFILE\.nez\archived_sessions -Recurse",
            "$env:userprofile/.nez/archived_sessions",
        ),
        (
            "sqlite3 \"$HOME/.nez/state_5.sqlite\" .tables",
            "$home/.nez/state_5.sqlite",
        ),
        ("ls $NEZ_HOME/sessions", "$nez_home/sessions"),
    ];
    for (command, expected) in cases {
        assert_eq!(
            find_internal_state_reference(command, &home()).as_deref(),
            Some(expected),
            "{command}"
        );
    }
}

#[test]
fn ordinary_commands_are_not_flagged() {
    for command in [
        "cat ~/.nez/config.toml",
        "ls ./sessions",
        "cargo test -p sessions",
        "echo ~/.nez",
    ] {
        assert_eq!(
            find_internal_state_reference(command, &home()),
            None,
            "{command}"
        );
    }
}

#[test]
fn custom_homes_only_match_their_own_path() {
    let custom = PathBuf::from("/data/nez-home");
    assert_eq!(
        find_internal_state_reference("cat ~/.nez/auth.json", &custom),
        None
    );
    assert_eq!(
        find_internal_state_reference("cat /data/nez-home/auth.json", &custom).as_deref(),
        Some("/data/nez-home/auth.json")
    );
}
