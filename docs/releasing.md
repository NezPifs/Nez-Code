# Releasing

Releases are built by the **Release** workflow when a `vX.Y.Z` tag is
pushed. The repository keeps version `0.0.0` in `nez-rs/Cargo.toml`; the
workflow stamps the tag's version into the workspace before building, so
source builds stay recognisable and `nez update` never offers to replace them.

1. Move the entries under `## [Unreleased]` in `CHANGELOG.md` into a new
   `## [X.Y.Z] - YYYY-MM-DD` section. The workflow uses that section as the
   release notes.
2. Merge the changelog update to `main` and wait for CI to pass.
3. Tag and push:

   ```sh
   git tag -a vX.Y.Z -m "Nez-Code X.Y.Z"
   git push origin vX.Y.Z
   ```

The workflow builds `nez`, `nez-code-mode-host` and the platform sandbox
helpers with `cargo build --locked --release` for:

| Platform | Runner | Archive |
| --- | --- | --- |
| `linux-x64` | ubuntu-22.04 | `nez-code-X.Y.Z-linux-x64.tar.gz` |
| `linux-arm64` | ubuntu-22.04-arm | `nez-code-X.Y.Z-linux-arm64.tar.gz` |
| `macos-arm64` | macos-14 | `nez-code-X.Y.Z-macos-arm64.tar.gz` |
| `macos-x64` | macos-14 (cross) | `nez-code-X.Y.Z-macos-x64.tar.gz` |
| `windows-x64` | windows-latest | `nez-code-X.Y.Z-windows-x64.zip` |

Each archive holds the binaries, `nez-resources/` (bubblewrap on Linux, the
sandbox helpers on Windows), `LICENSE`, `NOTICE` and `README.md`. The release
also carries `SHA256SUMS`, `install.sh` and `install.ps1`; the installers and
`nez update` download from the latest release and verify the checksum.

To rebuild an existing tag, run the workflow by hand with the tag as input.
