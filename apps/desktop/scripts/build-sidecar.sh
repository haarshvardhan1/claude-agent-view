#!/usr/bin/env bash
# Builds the `av` CLI and places it where Tauri expects the sidecar:
# src-tauri/binaries/av-<host triple>. Run from apps/desktop.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
triple="$(rustc -vV | sed -n 's/host: //p')"

cargo build --release -p av-cli --manifest-path "$root/Cargo.toml"
mkdir -p "$here/src-tauri/binaries"
dest="$here/src-tauri/binaries/av-$triple"
cp "$root/target/release/av" "$dest"
chmod 755 "$dest"
echo "sidecar: $dest"
