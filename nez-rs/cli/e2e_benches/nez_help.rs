#![allow(clippy::expect_used)]

use std::process::Command;

use divan::Bencher;

fn main() {
    divan::main();
}

/// Exercises the Bazel-backed end-to-end benchmark path with a cheap,
/// deterministic Nez invocation. Richer scenarios can add separate
/// benchmark binaries without making the shared harness depend on them.
#[divan::bench(sample_count = 20, sample_size = 1)]
fn nez_help(bencher: Bencher) {
    let nez = nez_utils_cargo_bin::cargo_bin("nez")
        .expect("nez binary should be available through Bazel runfiles");

    bencher.bench_local(move || {
        let output = Command::new(&nez)
            .arg("--help")
            .output()
            .expect("nez --help should run");
        assert!(output.status.success(), "nez --help should succeed");
    });
}
