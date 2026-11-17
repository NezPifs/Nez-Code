use super::*;
use pretty_assertions::assert_eq;

fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (path, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *data)
            .expect("append");
    }
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

fn zip_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, data) in entries {
        writer
            .start_file(*path, zip::write::SimpleFileOptions::default())
            .expect("start file");
        writer.write_all(data).expect("write");
    }
    writer.finish().expect("finish zip").into_inner()
}

#[test]
fn platforms_and_asset_names_follow_the_release_layout() {
    assert_eq!(platform_for("linux", "x86_64"), Some("linux-x64"));
    assert_eq!(platform_for("macos", "aarch64"), Some("macos-arm64"));
    assert_eq!(platform_for("windows", "x86_64"), Some("windows-x64"));
    assert_eq!(platform_for("freebsd", "x86_64"), None);
    let version = semver::Version::new(3, 0, 1);
    assert_eq!(
        asset_name(&version, "linux-x64"),
        "nez-code-3.0.1-linux-x64.tar.gz"
    );
    assert_eq!(
        asset_name(&version, "windows-x64"),
        "nez-code-3.0.1-windows-x64.zip"
    );
}

#[test]
fn tags_parse_with_or_without_a_v_prefix() {
    assert_eq!(
        version_from_tag("v3.0.0"),
        Some(semver::Version::new(3, 0, 0))
    );
    assert_eq!(
        version_from_tag("3.1.2"),
        Some(semver::Version::new(3, 1, 2))
    );
    assert_eq!(version_from_tag("nightly"), None);
}

#[test]
fn checksums_are_found_by_file_name() {
    let sums = "ABC123  nez-code-3.0.0-linux-x64.tar.gz\ndef456 *nez-code-3.0.0-windows-x64.zip\n";
    assert_eq!(
        checksum_for(sums, "nez-code-3.0.0-linux-x64.tar.gz").as_deref(),
        Some("abc123")
    );
    assert_eq!(
        checksum_for(sums, "nez-code-3.0.0-windows-x64.zip").as_deref(),
        Some("def456")
    );
    assert_eq!(checksum_for(sums, "other"), None);
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn tarballs_yield_the_binary_and_resources() {
    let archive = tar_gz(&[
        ("nez-code-3.0.0-linux-x64/nez", b"binary"),
        ("nez-code-3.0.0-linux-x64/nez-code-mode-host", b"host"),
        ("nez-code-3.0.0-linux-x64/nez-resources/bwrap", b"helper"),
        ("nez-code-3.0.0-linux-x64/README.md", b"docs"),
    ]);
    let unpacked = unpack(&archive, /*windows*/ false).expect("unpack");
    assert_eq!(unpacked.binary, b"binary");
    assert_eq!(
        unpacked.helpers,
        vec![("nez-code-mode-host".to_string(), b"host".to_vec())]
    );
    assert_eq!(
        unpacked.resources,
        vec![(PathBuf::from("bwrap"), b"helper".to_vec())]
    );
}

#[test]
fn zips_yield_the_windows_binary() {
    let archive = zip_archive(&[("nez.exe", b"exe"), ("LICENSE", b"text")]);
    let unpacked = unpack(&archive, /*windows*/ true).expect("unpack");
    assert_eq!(unpacked.binary, b"exe");
    assert!(unpack(&zip_archive(&[("LICENSE", b"text")]), true).is_err());
}

#[test]
fn install_keeps_the_previous_binary_and_rollback_restores_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let current = dir.path().join("nez");
    std::fs::write(&current, b"old").expect("write");

    install(
        &current,
        &Unpacked {
            binary: b"new".to_vec(),
            helpers: vec![("nez-code-mode-host".to_string(), b"host".to_vec())],
            resources: vec![(PathBuf::from("bwrap"), b"helper".to_vec())],
        },
    )
    .expect("install");
    assert_eq!(std::fs::read(&current).expect("read"), b"new");
    assert_eq!(
        std::fs::read(dir.path().join("nez.previous")).expect("read"),
        b"old"
    );
    assert_eq!(
        std::fs::read(dir.path().join("nez-resources/bwrap")).expect("read"),
        b"helper"
    );
    assert_eq!(
        std::fs::read(dir.path().join("nez-code-mode-host")).expect("read"),
        b"host"
    );

    rollback(&current).expect("rollback");
    assert_eq!(std::fs::read(&current).expect("read"), b"old");
    assert_eq!(
        std::fs::read(dir.path().join("nez.previous")).expect("read"),
        b"new"
    );
}

#[test]
fn rollback_without_a_previous_version_fails_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let current = dir.path().join("nez.exe");
    std::fs::write(&current, b"only").expect("write");
    assert_eq!(previous_path(&current), dir.path().join("nez.previous.exe"));
    assert!(rollback(&current).is_err());
    assert_eq!(std::fs::read(&current).expect("read"), b"only");
}
