#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS universal packaging must run on macOS." >&2
  exit 1
fi

if ! command -v lipo >/dev/null 2>&1; then
  echo "lipo is required to create a macOS universal binary." >&2
  exit 1
fi

if ! cargo packager --version >/dev/null 2>&1; then
  echo "cargo-packager is required. Install it with: cargo install cargo-packager --locked" >&2
  exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin

cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

universal_dir="target/universal-apple-darwin/release"
mkdir -p "$universal_dir"

lipo -create \
  target/aarch64-apple-darwin/release/velocimd \
  target/x86_64-apple-darwin/release/velocimd \
  -output "$universal_dir/velocimd"

chmod +x "$universal_dir/velocimd"
lipo -info "$universal_dir/velocimd"

packager_config="$(mktemp "$PWD/.packager-macos-universal.XXXXXX.toml")"
trap 'rm -f "$packager_config"' EXIT

awk '
  BEGIN {
    target_triple_inserted = 0
  }
  function insert_target_triple() {
    if (!target_triple_inserted) {
      print "target-triple = \"universal-apple-darwin\""
      target_triple_inserted = 1
    }
  }
  /^binaries-dir[[:space:]]*=/ {
    print "binaries-dir = \"target/universal-apple-darwin/release\""
    insert_target_triple()
    next
  }
  /^before-packaging-command[[:space:]]*=/ {
    next
  }
  /^target-triple[[:space:]]*=/ {
    next
  }
  /^\[/ {
    insert_target_triple()
  }
  { print }
  END {
    insert_target_triple()
  }
' Packager.toml > "$packager_config"

cargo packager --config "$packager_config"
