#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    nez_windows_sandbox::setup_helper_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("nez-windows-sandbox-setup is Windows-only");
}
