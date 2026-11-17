#!/bin/sh
# Installs the latest Nez-Code release for macOS or Linux, then offers to set
# up Ollama with a model sized for this machine.
#
#   curl -fsSL https://github.com/NezPifs/Nez-Code/releases/latest/download/install.sh | sh
#
# Environment:
#   NEZ_VERSION          release tag to install (default: latest)
#   NEZ_INSTALL_DIR      where to put the binary (default: ~/.nez/bin)
#   NEZ_NON_INTERACTIVE  1 = answer every question with its default
#   NEZ_SKIP_OLLAMA      1 = do not touch Ollama or models
#   GITHUB_TOKEN         token for private repositories or rate limits
set -eu

REPO="NezPifs/Nez-Code"
INSTALL_DIR="${NEZ_INSTALL_DIR:-$HOME/.nez/bin}"
VERSION="${NEZ_VERSION:-latest}"
NON_INTERACTIVE="${NEZ_NON_INTERACTIVE:-0}"
SKIP_OLLAMA="${NEZ_SKIP_OLLAMA:-0}"
TOKEN="${NEZ_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
RELEASES_API="${NEZ_RELEASES_API:-https://api.github.com/repos/$REPO/releases}"
DEFAULT_MODEL="qwen3.5:9b"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

ask() {
    # ask "Question" default -> prints the answer
    if [ "$NON_INTERACTIVE" = "1" ] || [ ! -r /dev/tty ]; then
        printf '%s\n' "$2"
        return
    fi
    printf '%s [%s]: ' "$1" "$2" >/dev/tty
    read -r answer </dev/tty || answer=""
    printf '%s\n' "${answer:-$2}"
}

fetch() {
    # fetch URL OUTPUT [ACCEPT]
    if [ -n "$TOKEN" ]; then
        curl -fsSL -H "Authorization: Bearer $TOKEN" -H "Accept: ${3:-application/vnd.github+json}" -o "$2" "$1"
    else
        curl -fsSL -H "Accept: ${3:-application/vnd.github+json}" -o "$2" "$1"
    fi
}

platform() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) die "unsupported operating system: $os (use install.ps1 on Windows)" ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch=x64 ;;
        arm64 | aarch64) arch=arm64 ;;
        *) die "unsupported CPU architecture: $arch" ;;
    esac
    printf '%s-%s\n' "$os" "$arch"
}

# Prints "<asset api url> <name>" for each release asset. The API returns
# compact JSON, so split it into one field per line first.
release_assets() {
    # shellcheck disable=SC2020 # every listed character becomes a newline
    tr ',{}' '\n\n\n' <"$1" | awk '
        /"url": *"[^"]*\/releases\/assets\/[0-9]+"/ { match($0, /https?:[^"]*/); url = substr($0, RSTART, RLENGTH) }
        /"name": *"/ && url != "" { match($0, /"name": *"[^"]*"/); n = substr($0, RSTART, RLENGTH); sub(/"name": *"/, "", n); sub(/"$/, "", n); print url " " n; url = "" }
    '
}

gpu_memory_mib() {
    if has nvidia-smi; then
        nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null | sort -n | tail -1 && return
    fi
    for f in /sys/class/drm/card*/device/mem_info_vram_total; do
        [ -r "$f" ] && { echo $(( $(cat "$f") / 1048576 )); return; }
    done
    if [ "$(uname -s)" = "Darwin" ] && [ "$(uname -m)" = "arm64" ]; then
        # Apple Silicon shares memory; about two thirds is usable by the GPU.
        echo $(( $(sysctl -n hw.memsize) * 2 / 3 / 1048576 ))
        return
    fi
    echo 0
}

recommended_model() {
    mib=$(gpu_memory_mib)
    gib=$(( (mib + 512) / 1024 ))
    if [ "$mib" -eq 0 ]; then echo "$DEFAULT_MODEL"
    elif [ "$gib" -ge 12 ]; then echo "deepseek-coder-v2:16b"
    elif [ "$gib" -ge 8 ]; then echo "$DEFAULT_MODEL"
    else echo "qwen2.5-coder:7b-instruct"
    fi
}

install_nez() {
    has curl || die "curl is required"
    has tar || die "tar is required"
    plat=$(platform)
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    if [ "$VERSION" = "latest" ]; then
        api="$RELEASES_API/latest"
    else
        api="$RELEASES_API/tags/$VERSION"
    fi
    fetch "$api" "$tmp/release.json" || die "could not read release information from GitHub"
    tag=$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/release.json" | head -1)
    [ -n "$tag" ] || die "no release found"
    version=${tag#v}
    asset="nez-code-$version-$plat.tar.gz"

    release_assets "$tmp/release.json" >"$tmp/assets.txt"
    asset_url=$(awk -v n="$asset" '$2 == n { print $1 }' "$tmp/assets.txt")
    sums_url=$(awk '$2 == "SHA256SUMS" { print $1 }' "$tmp/assets.txt")
    [ -n "$asset_url" ] || die "release $tag has no build for $plat"

    say "Downloading Nez-Code $version for $plat..."
    fetch "$asset_url" "$tmp/$asset" application/octet-stream || die "download failed"
    if [ -n "$sums_url" ]; then
        fetch "$sums_url" "$tmp/SHA256SUMS" application/octet-stream || die "could not download checksums"
        expected=$(awk -v n="$asset" '$2 == n { print $1 }' "$tmp/SHA256SUMS")
        if has sha256sum; then actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
        else actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1); fi
        [ "$expected" = "$actual" ] || die "checksum mismatch for $asset"
    fi

    mkdir -p "$tmp/unpacked" "$INSTALL_DIR"
    tar -xzf "$tmp/$asset" -C "$tmp/unpacked"
    [ -f "$INSTALL_DIR/nez" ] && mv -f "$INSTALL_DIR/nez" "$INSTALL_DIR/nez.previous"
    mv -f "$tmp/unpacked/nez" "$INSTALL_DIR/nez"
    chmod +x "$INSTALL_DIR/nez"
    if [ -f "$tmp/unpacked/nez-code-mode-host" ]; then
        mv -f "$tmp/unpacked/nez-code-mode-host" "$INSTALL_DIR/nez-code-mode-host"
        chmod +x "$INSTALL_DIR/nez-code-mode-host"
    fi
    if [ -d "$tmp/unpacked/nez-resources" ]; then
        rm -rf "$INSTALL_DIR/nez-resources"
        mv "$tmp/unpacked/nez-resources" "$INSTALL_DIR/nez-resources"
    fi
    say "Installed $INSTALL_DIR/nez"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            line="export PATH=\"$INSTALL_DIR:\$PATH\""
            for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
                if [ -f "$rc" ] || [ "$rc" = "$HOME/.profile" ]; then
                    grep -qsF "$line" "$rc" || printf '\n%s\n' "$line" >>"$rc"
                fi
            done
            say "Added $INSTALL_DIR to PATH in your shell profile. Open a new terminal, or run:"
            say "  $line"
            ;;
    esac
}

setup_ollama() {
    [ "$SKIP_OLLAMA" = "1" ] && return
    if ! has ollama; then
        answer=$(ask "Install Ollama to run models locally?" "Y")
        case "$answer" in [Nn]*) say "Skipping Ollama. Run \`nez setup\` to use the DeepSeek API instead."; return ;; esac
        if [ "$(uname -s)" = "Darwin" ]; then
            if has brew; then brew install ollama
            else say "Install Ollama from https://ollama.com/download, then run \`nez setup\`."; return; fi
        else
            curl -fsSL https://ollama.com/install.sh | sh
        fi
    fi
    if ! curl -fsS http://127.0.0.1:11434/api/version >/dev/null 2>&1; then
        (ollama serve >/dev/null 2>&1 &)
        i=0
        while [ $i -lt 30 ] && ! curl -fsS http://127.0.0.1:11434/api/version >/dev/null 2>&1; do
            sleep 1; i=$((i + 1))
        done
    fi
    model=$(ask "Model to download" "$(recommended_model)")
    say "Downloading $model (this can take a while)..."
    ollama pull "$model" || { say "Could not download $model. Run \`ollama pull $model\` later."; return; }

    config="${NEZ_HOME:-$HOME/.nez}/config.toml"
    mkdir -p "$(dirname "$config")"
    if ! grep -qs '^model_provider' "$config"; then
        { printf 'model_provider = "ollama"\nmodel = "%s"\n' "$model"; [ -f "$config" ] && cat "$config"; } >"$config.new"
        mv "$config.new" "$config"
        : >"$(dirname "$config")/.setup-complete"
    fi
}

if [ -n "${NEZ_INSTALL_IF_LATEST:-}" ]; then
    # The app-server daemon's managed updater speaks a different protocol.
    die "managed daemon installs are not supported; run \`nez update\` instead"
fi

install_nez
setup_ollama
say ""
say "Done. Run \`nez\` in a project folder to start."
