//! Best-effort detection of GPU memory for picking a default model tier.
//!
//! Every probe is optional: a missing tool or unreadable file just moves on to
//! the next source, and no answer at all falls back to the default model.

use nez_model_profiles::HardwareTier;
use nez_model_profiles::recommended_model_for_vram;
use std::process::Command;
use std::time::Duration;

/// Where the VRAM figure came from, for display during setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuMemory {
    pub mib: u64,
    pub source: &'static str,
}

/// Largest GPU memory found on this machine.
pub fn detect_gpu_memory() -> Option<GpuMemory> {
    nvidia_smi()
        .or_else(amd_sysfs)
        .or_else(apple_unified_memory)
        .or_else(windows_adapter_memory)
}

/// Recommended tier for this machine.
pub fn recommended_tier() -> (HardwareTier, Option<GpuMemory>) {
    let memory = detect_gpu_memory();
    (
        recommended_model_for_vram(memory.as_ref().map(|m| m.mib)),
        memory,
    )
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // Hardware tools can hang on broken drivers; never block setup on them.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8(output.stdout).ok()
}

/// `nvidia-smi --query-gpu=memory.total` prints one MiB value per GPU.
pub(crate) fn parse_nvidia_smi(output: &str) -> Option<u64> {
    output
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .max()
}

fn nvidia_smi() -> Option<GpuMemory> {
    let output = run(
        "nvidia-smi",
        &["--query-gpu=memory.total", "--format=csv,noheader,nounits"],
    )?;
    parse_nvidia_smi(&output).map(|mib| GpuMemory {
        mib,
        source: "nvidia-smi",
    })
}

fn amd_sysfs() -> Option<GpuMemory> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            std::fs::read_to_string(entry.path().join("device/mem_info_vram_total")).ok()
        })
        .filter_map(|bytes| bytes.trim().parse::<u64>().ok())
        .max()
        .map(|bytes| GpuMemory {
            mib: bytes / (1024 * 1024),
            source: "amdgpu",
        })
}

/// Apple Silicon shares system memory with the GPU. macOS lets the GPU wire
/// roughly two thirds of it by default, which is what a model can use.
pub(crate) fn apple_usable_mib(total_bytes: u64) -> u64 {
    total_bytes / (1024 * 1024) * 2 / 3
}

fn apple_unified_memory() -> Option<GpuMemory> {
    if !(cfg!(target_os = "macos") && cfg!(target_arch = "aarch64")) {
        return None;
    }
    let output = run("sysctl", &["-n", "hw.memsize"])?;
    let bytes = output.trim().parse::<u64>().ok()?;
    Some(GpuMemory {
        mib: apple_usable_mib(bytes),
        source: "unified memory",
    })
}

/// Parses `HardwareInformation.qwMemorySize` values printed one per line.
/// The WMI `AdapterRAM` field is a 32-bit value and caps at 4 GiB, so the
/// registry is the reliable source on Windows.
pub(crate) fn parse_windows_qw_memory(output: &str) -> Option<u64> {
    output
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .max()
        .map(|bytes| bytes / (1024 * 1024))
}

fn windows_adapter_memory() -> Option<GpuMemory> {
    if !cfg!(windows) {
        return None;
    }
    let script = "Get-ItemProperty -Path 'HKLM:\\SYSTEM\\ControlSet001\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}\\0*' -ErrorAction SilentlyContinue | ForEach-Object { $_.'HardwareInformation.qwMemorySize' }";
    let output = run(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;
    parse_windows_qw_memory(&output).map(|mib| GpuMemory {
        mib,
        source: "display adapter",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn nvidia_output_takes_the_largest_gpu() {
        assert_eq!(parse_nvidia_smi("8192\n24576\n"), Some(24_576));
        assert_eq!(parse_nvidia_smi("N/A\n"), None);
    }

    #[test]
    fn windows_registry_values_are_bytes() {
        assert_eq!(parse_windows_qw_memory("8589934592\r\n\r\n"), Some(8_192));
    }

    #[test]
    fn apple_memory_reserves_a_third_for_the_system() {
        let sixteen_gib = 16 * 1024 * 1024 * 1024;
        assert_eq!(apple_usable_mib(sixteen_gib), 10_922);
        assert_eq!(
            recommended_model_for_vram(Some(apple_usable_mib(sixteen_gib))).model,
            "qwen3.5:9b"
        );
    }
}
