#!/bin/sh
# Exercises scripts/install.sh against a local stand-in for the GitHub
# releases API: a fresh install, a reinstall that keeps the previous binary,
# a checksum mismatch and a refused daemon invocation.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
port=${NEZ_TEST_PORT:-8766}
trap 'kill "$server" 2>/dev/null || true; rm -rf "$work"' EXIT

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) platform=linux-x64 ;;
    Linux-aarch64) platform=linux-arm64 ;;
    Darwin-arm64) platform=macos-arm64 ;;
    Darwin-x86_64) platform=macos-x64 ;;
    *) echo "unsupported test platform"; exit 1 ;;
esac
asset="nez-code-9.9.9-$platform.tar.gz"

mkdir -p "$work/stage/nez-resources" "$work/srv/releases/assets" "$work/home"
printf '#!/bin/sh\necho "nez 9.9.9"\n' >"$work/stage/nez"
printf '#!/bin/sh\n' >"$work/stage/nez-code-mode-host"
chmod +x "$work/stage/nez" "$work/stage/nez-code-mode-host"
printf 'helper' >"$work/stage/nez-resources/bwrap"
tar -C "$work/stage" -czf "$work/srv/releases/assets/1" .
if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum "$work/srv/releases/assets/1" | cut -d' ' -f1)
else sum=$(shasum -a 256 "$work/srv/releases/assets/1" | cut -d' ' -f1); fi
printf '%s  %s\n' "$sum" "$asset" >"$work/srv/releases/assets/2"
printf '{"tag_name":"v9.9.9","assets":[{"url":"http://127.0.0.1:%s/releases/assets/1","id":1,"name":"%s"},{"url":"http://127.0.0.1:%s/releases/assets/2","id":2,"name":"SHA256SUMS"}]}' \
    "$port" "$asset" "$port" >"$work/srv/releases/latest"

(cd "$work/srv" && exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1) &
server=$!
i=0
until curl -fsS "http://127.0.0.1:$port/releases/latest" >/dev/null 2>&1; do
    i=$((i + 1)); [ $i -lt 50 ] || { echo "test server did not start"; exit 1; }; sleep 0.1
done

install() {
    env HOME="$work/home" NEZ_RELEASES_API="http://127.0.0.1:$port/releases" \
        NEZ_SKIP_OLLAMA=1 NEZ_NON_INTERACTIVE=1 "$@" sh "$here/install.sh"
}
fail() { echo "FAIL: $*"; exit 1; }
bin="$work/home/.nez/bin"

install >/dev/null || fail "fresh install exited with $?"
[ "$("$bin/nez")" = "nez 9.9.9" ] || fail "nez was not installed"
[ -x "$bin/nez-code-mode-host" ] || fail "nez-code-mode-host was not installed"
[ -f "$bin/nez-resources/bwrap" ] || fail "nez-resources were not installed"
[ "$(grep -c "$bin" "$work/home/.profile")" = 1 ] || fail "PATH was not added to .profile once"

install >/dev/null || fail "reinstall exited with $?"
[ -f "$bin/nez.previous" ] || fail "reinstall did not keep nez.previous"
[ "$(grep -c "$bin" "$work/home/.profile")" = 1 ] || fail "reinstall added PATH twice"

echo "0000  $asset" >"$work/srv/releases/assets/2"
if install NEZ_INSTALL_DIR="$work/other" >/dev/null 2>&1; then fail "checksum mismatch was accepted"; fi
[ ! -e "$work/other/nez" ] || fail "a mismatched download was installed"

if install NEZ_INSTALL_IF_LATEST=1 >/dev/null 2>&1; then fail "daemon invocation was accepted"; fi

echo "install.sh: all checks passed"
