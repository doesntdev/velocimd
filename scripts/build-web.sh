#!/usr/bin/env bash
# Standalone real egui app; assets work at / or under a same-origin subdirectory.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(python3 -c 'import tomllib; d=tomllib.load(open("Cargo.lock","rb")); print(next(p["version"] for p in d["package"] if p["name"]=="wasm-bindgen"))')
bindgen=${WASM_BINDGEN:-wasm-bindgen}
if [[ "$("$bindgen" --version)" != "wasm-bindgen $version" ]]; then
  printf 'Install the locked CLI first: cargo install wasm-bindgen-cli --version %s --locked\n' "$version" >&2
  exit 1
fi
# Isolate the qualified browser compiler policy from native release tuning.
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_OPT_LEVEL=3
cargo build --locked --release --lib --target wasm32-unknown-unknown
mkdir -p target/web
target_dir="$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
"$bindgen" --target web --out-dir target/web --out-name velocimd "$target_dir/wasm32-unknown-unknown/release/velocimd.wasm"
python3 scripts/strip_wasm.py target/web/velocimd_bg.wasm target/web/velocimd_bg.wasm > target/web/wasm-strip.json
python3 scripts/build_web_policy.py Cargo.toml target/web/wasm-strip.json "$(command -v "$bindgen")" > target/web/build-policy.json
cp index.html target/web/index.html
cp web/browser-store.js target/web/browser-store.js
printf 'Built target/web/index.html, target/web/velocimd.js, target/web/velocimd_bg.wasm, target/web/browser-store.js\n'
