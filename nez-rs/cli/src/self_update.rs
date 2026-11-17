//! `nez update`: replaces this binary with a release published on GitHub.
//!
//! Releases carry one archive per platform (`nez-code-<version>-<platform>`,
//! `.tar.gz` or `.zip` on Windows) and a `SHA256SUMS` file. The running
//! binary is kept next to the new one as `nez.previous` so `--rollback` can
//! restore it.

use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use nez_http_client::ClientRouteClass;
use nez_http_client::HttpClientFactory;
use nez_http_client::OutboundProxyPolicy;
use nez_login::default_client::ClientRedirectPolicy;
use nez_login::default_client::create_client_for_route_async;
use serde::Deserialize;
use sha2::Digest;

pub const REPO: &str = "NezPifs/Nez-Code";
const RESOURCES_DIR: &str = "nez-resources";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateOptions {
    /// Install without asking.
    pub yes: bool,
    /// Only report whether an update is available.
    pub check: bool,
    /// Restore the binary that the last update replaced.
    pub rollback: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Asset {
    pub name: String,
    /// API URL; returns the file with `Accept: application/octet-stream` and
    /// works for private repositories with a token.
    pub url: String,
}

/// The release platform name for this build, if releases cover it.
pub(crate) fn platform() -> Option<&'static str> {
    platform_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub(crate) fn platform_for(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("windows", "x86_64") => "windows-x64",
        _ => return None,
    })
}

pub(crate) fn asset_name(version: &semver::Version, platform: &str) -> String {
    let extension = if platform.starts_with("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("nez-code-{version}-{platform}.{extension}")
}

pub(crate) fn version_from_tag(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// The expected SHA-256 for `name` in a `SHA256SUMS` file.
pub(crate) fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name).then(|| digest.to_ascii_lowercase())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Files unpacked from a release archive.
#[derive(Debug, Default)]
pub(crate) struct Unpacked {
    pub binary: Vec<u8>,
    /// Helper executables that live next to `nez`, by file name.
    pub helpers: Vec<(String, Vec<u8>)>,
    /// Paths relative to `nez-resources/`, with their contents.
    pub resources: Vec<(PathBuf, Vec<u8>)>,
}

fn binary_name(windows: bool) -> &'static str {
    if windows { "nez.exe" } else { "nez" }
}

/// Helpers the CLI starts from its own directory.
fn helper_names(windows: bool) -> &'static [&'static str] {
    if windows {
        &["nez-code-mode-host.exe"]
    } else {
        &["nez-code-mode-host"]
    }
}

fn classify(path: &Path, windows: bool, data: Vec<u8>, out: &mut Unpacked) {
    let components: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    // Archives may or may not wrap their contents in a top-level folder.
    if let Some(index) = components.iter().position(|c| c == RESOURCES_DIR) {
        let relative: PathBuf = components[index + 1..].iter().collect();
        if !relative.as_os_str().is_empty() {
            out.resources.push((relative, data));
        }
    } else if let Some(name) = components.last() {
        if name == binary_name(windows) {
            out.binary = data;
        } else if helper_names(windows).contains(&name.as_str()) {
            out.helpers.push((name.clone(), data));
        }
    }
}

pub(crate) fn unpack(archive: &[u8], windows: bool) -> anyhow::Result<Unpacked> {
    let mut out = Unpacked::default();
    if windows {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let Some(path) = file.enclosed_name() else {
                continue;
            };
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            classify(&path, windows, data, &mut out);
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path()?.into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            classify(&path, windows, data, &mut out);
        }
    }
    if out.binary.is_empty() {
        bail!(
            "the release archive does not contain {}",
            binary_name(windows)
        );
    }
    Ok(out)
}

/// Where the replaced binary is kept: `nez.previous` (`nez.previous.exe`).
pub(crate) fn previous_path(current: &Path) -> PathBuf {
    let stem = current
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "nez".to_string());
    let name = match current.extension() {
        Some(ext) => format!("{stem}.previous.{}", ext.to_string_lossy()),
        None => format!("{stem}.previous"),
    };
    current.with_file_name(name)
}

/// Installs `unpacked` over `current`, keeping the old binary for rollback.
/// Renaming the running executable works on every platform, including
/// Windows, so the binary is swapped by rename rather than overwritten.
pub(crate) fn install(current: &Path, unpacked: &Unpacked) -> anyhow::Result<()> {
    let dir = current
        .parent()
        .context("the current executable has no parent directory")?;
    let staged = dir.join(format!(".nez-update-{}", std::process::id()));
    std::fs::write(&staged, &unpacked.binary)
        .with_context(|| format!("cannot write to {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    let previous = previous_path(current);
    if previous.exists() {
        std::fs::remove_file(&previous)?;
    }
    std::fs::rename(current, &previous)
        .with_context(|| format!("cannot move {} aside", current.display()))?;
    if let Err(err) = std::fs::rename(&staged, current) {
        // Put the original back so the user is never left without a binary.
        let _ = std::fs::rename(&previous, current);
        let _ = std::fs::remove_file(&staged);
        return Err(err).context("cannot install the new binary");
    }
    for (name, data) in &unpacked.helpers {
        // Helpers are not running during an update, so they can be overwritten.
        let path = dir.join(name);
        std::fs::write(&path, data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    if !unpacked.resources.is_empty() {
        let resources = dir.join(RESOURCES_DIR);
        if resources.exists() {
            std::fs::remove_dir_all(&resources)?;
        }
        for (relative, data) in &unpacked.resources {
            let path = resources.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    Ok(())
}

/// Swaps the current binary with the one the last update replaced.
pub(crate) fn rollback(current: &Path) -> anyhow::Result<()> {
    let previous = previous_path(current);
    if !previous.exists() {
        bail!(
            "there is no previous version to restore ({} not found)",
            previous.display()
        );
    }
    let staged = current.with_file_name(format!(".nez-rollback-{}", std::process::id()));
    std::fs::rename(current, &staged)?;
    if let Err(err) = std::fs::rename(&previous, current) {
        let _ = std::fs::rename(&staged, current);
        return Err(err).context("cannot restore the previous binary");
    }
    std::fs::rename(&staged, &previous)?;
    Ok(())
}

fn github_token() -> Option<String> {
    ["NEZ_GITHUB_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
}

async fn get(url: &str, accept: &str) -> anyhow::Result<Vec<u8>> {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    let client = create_client_for_route_async(
        factory,
        url.to_string(),
        ClientRouteClass::Other,
        ClientRedirectPolicy::Default,
    )
    .await?;
    let mut request = client
        .get(url)
        .header("Accept", accept)
        .header("User-Agent", "nez-updater")
        .timeout(REQUEST_TIMEOUT);
    if let Some(token) = github_token() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .and_then(nez_http_client::HttpResponse::error_for_status)
        .with_context(|| format!("request to {url} failed"))?;
    Ok(response.bytes().await?.to_vec())
}

async fn latest_release() -> anyhow::Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = get(&url, "application/vnd.github+json").await.context(
        "could not read the latest release; set GITHUB_TOKEN if the repository is private",
    )?;
    Ok(serde_json::from_slice(&body)?)
}

fn confirm(question: &str) -> anyhow::Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("run `nez update --yes` to update without a prompt");
    }
    print!("{question} [Y/n] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "" | "y" | "Y" | "yes"))
}

pub async fn run(options: UpdateOptions) -> anyhow::Result<()> {
    let current_exe = std::env::current_exe()?;
    if options.rollback {
        rollback(&current_exe)?;
        println!("Restored the previous Nez-Code version. Run `nez --version` to check it.");
        return Ok(());
    }
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    if current == semver::Version::new(0, 0, 0) {
        bail!("this is a development build; install a release with the install script instead");
    }
    let Some(platform) = platform() else {
        bail!("there are no release builds for this platform");
    };
    let release = latest_release().await?;
    let latest = version_from_tag(&release.tag_name)
        .with_context(|| format!("unexpected release tag `{}`", release.tag_name))?;
    if latest <= current {
        println!("Nez-Code {current} is up to date.");
        return Ok(());
    }
    if options.check {
        println!(
            "Nez-Code {latest} is available (you have {current}). Run `nez update` to install it."
        );
        return Ok(());
    }
    if !options.yes && !confirm(&format!("Update Nez-Code {current} to {latest}?"))? {
        return Ok(());
    }
    let name = asset_name(&latest, platform);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .with_context(|| format!("release {} has no {name}", release.tag_name))?;
    println!("Downloading {name}...");
    let archive = get(&asset.url, "application/octet-stream").await?;
    if let Some(sums) = release.assets.iter().find(|a| a.name == "SHA256SUMS") {
        let sums = String::from_utf8(get(&sums.url, "application/octet-stream").await?)?;
        let expected = checksum_for(&sums, &name)
            .with_context(|| format!("SHA256SUMS has no entry for {name}"))?;
        if sha256_hex(&archive) != expected {
            bail!("checksum mismatch for {name}; the download was not installed");
        }
    }
    let unpacked = unpack(&archive, platform.starts_with("windows"))?;
    install(&current_exe, &unpacked)?;
    println!("Updated to Nez-Code {latest}. `nez update --rollback` restores {current} if needed.");
    Ok(())
}

#[cfg(test)]
#[path = "self_update_tests.rs"]
mod tests;
