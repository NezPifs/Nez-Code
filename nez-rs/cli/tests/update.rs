use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn nez_command(nez_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(nez_utils_cargo_bin::cargo_bin("nez")?);
    cmd.env("NEZ_HOME", nez_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn update_does_not_start_interactive_prompt() -> Result<()> {
    let nez_home = TempDir::new()?;

    nez_command(nez_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("this is a development build"));

    Ok(())
}
