//! Seatbelt keeps deny globs as native patterns; no snapshot scan is needed.

use super::dependencies::Dependency;
use super::dependencies::nez_executable;
use nez_protocol::permissions::FileSystemSandboxPolicy;
use nez_sandboxing::SandboxExecRequest;
use nez_sandboxing::SandboxType;
use nez_utils_absolute_path::AbsolutePathBuf;
use std::io;
use std::path::PathBuf;

pub(super) const SANDBOX_TYPE: SandboxType = SandboxType::MacosSeatbelt;
pub(super) const NAME: &str = "seatbelt";

pub(super) fn critical_dependencies(
    _request: &SandboxExecRequest,
    _policy: &FileSystemSandboxPolicy,
    _cwd: &AbsolutePathBuf,
) -> io::Result<Vec<Dependency>> {
    Ok(vec![
        nez_executable(),
        (
            "seatbelt",
            Ok(PathBuf::from(
                nez_sandboxing::seatbelt::MACOS_PATH_TO_SEATBELT_EXECUTABLE,
            )),
        ),
    ])
}

pub(super) fn prepare_policy(
    _request: &SandboxExecRequest,
    policy: FileSystemSandboxPolicy,
    _cwd: &AbsolutePathBuf,
) -> io::Result<FileSystemSandboxPolicy> {
    Ok(policy)
}
