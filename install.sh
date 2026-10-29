#!/bin/sh
# Installs the latest skillster release on Linux or macOS:
#   curl -fsSL https://github.com/alexcrisan1611/skillster/releases/latest/download/install.sh | sh
# Extra arguments go to `skillster install`:
#   curl -fsSL .../install.sh | sh -s -- --config ~/my-config.toml
set -eu

repo="alexcrisan1611/skillster"
base="https://github.com/$repo/releases/latest/download"

case "$(uname -s)" in
    Linux) os="unknown-linux-musl" ;;
    Darwin) os="apple-darwin" ;;
    *) echo "skillster: unsupported OS $(uname -s); on Windows use install.ps1" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="aarch64" ;;
    *) echo "skillster: unsupported CPU $(uname -m)" >&2; exit 1 ;;
esac
name="skillster-$arch-$os"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $name..."
curl -fsSL "$base/$name.tar.gz" -o "$tmp/$name.tar.gz"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"

expected="$(grep " $name.tar.gz\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/$name.tar.gz" | cut -d' ' -f1)"
else
    actual="$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1)"
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    echo "skillster: checksum mismatch for $name.tar.gz" >&2
    exit 1
fi

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
# The installer asks questions; read the answers from the terminal, not from this pipe.
"$tmp/$name/skillster" install "$@" </dev/tty
