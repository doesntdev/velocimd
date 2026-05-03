#!/usr/bin/env bash
set -euo pipefail

repo="doesntdev/velocimd"
app_name="Velocimd"
install_dir="${VELOCIMD_INSTALL_DIR:-$HOME/Applications}"
api_url="https://api.github.com/repos/$repo/releases/latest"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer is only for macOS." >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required." >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to read GitHub release metadata." >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
mount_dir="$tmpdir/mount"
release_json="$tmpdir/release.json"
dmg_path="$tmpdir/$app_name.dmg"
mkdir -p "$mount_dir"

detach_dmg() {
  if mount | grep -q "on $mount_dir "; then
    hdiutil detach "$mount_dir" -quiet >/dev/null 2>&1 || true
  fi
}

cleanup() {
  detach_dmg
  rm -rf "$tmpdir"
}
trap cleanup EXIT

echo "Fetching latest Velocimd release metadata..."
curl -fsSL "$api_url" -o "$release_json"

asset_info="$(
  python3 - "$release_json" "$(uname -m)" <<'PY'
import json
import sys

release_path, machine = sys.argv[1], sys.argv[2]
with open(release_path, "r", encoding="utf-8") as handle:
    release = json.load(handle)

assets = release.get("assets", [])
dmgs = [
    asset for asset in assets
    if asset.get("name", "").lower().endswith(".dmg")
]

if machine in {"arm64", "aarch64"}:
    preferred = ("universal", "aarch64", "arm64")
elif machine == "x86_64":
    preferred = ("universal", "x86_64", "x64")
else:
    raise SystemExit(f"Unsupported macOS architecture: {machine}")

for token in preferred:
    for asset in dmgs:
        name = asset.get("name", "")
        if token in name:
            print(asset["name"])
            print(asset["browser_download_url"])
            print(asset.get("digest", ""))
            raise SystemExit(0)

available = ", ".join(asset.get("name", "") for asset in dmgs) or "none"
raise SystemExit(f"No compatible macOS DMG found for {machine}. Available DMGs: {available}")
PY
)"

asset_name="$(printf '%s\n' "$asset_info" | sed -n '1p')"
asset_url="$(printf '%s\n' "$asset_info" | sed -n '2p')"
asset_digest="$(printf '%s\n' "$asset_info" | sed -n '3p')"

echo "Downloading $asset_name..."
curl -fL "$asset_url" -o "$dmg_path"

if [[ "$asset_digest" == sha256:* ]]; then
  expected_sha="${asset_digest#sha256:}"
  actual_sha="$(shasum -a 256 "$dmg_path" | awk '{print $1}')"
  if [[ "$actual_sha" != "$expected_sha" ]]; then
    echo "Checksum mismatch for $asset_name." >&2
    echo "Expected: $expected_sha" >&2
    echo "Actual:   $actual_sha" >&2
    exit 1
  fi
  echo "Verified SHA-256 checksum."
else
  echo "No SHA-256 digest found in release metadata; refusing to install." >&2
  exit 1
fi

echo "Mounting $asset_name..."
hdiutil attach "$dmg_path" -nobrowse -quiet -mountpoint "$mount_dir"

app_source="$(find "$mount_dir" -maxdepth 2 -type d -name "$app_name.app" -print -quit)"
if [[ -z "$app_source" ]]; then
  echo "$app_name.app was not found in the DMG." >&2
  exit 1
fi

mkdir -p "$install_dir"
app_target="$install_dir/$app_name.app"

echo "Installing to $app_target..."
rm -rf "$app_target"
ditto "$app_source" "$app_target"

echo "Removing quarantine metadata from $app_target..."
xattr -dr com.apple.quarantine "$app_target" >/dev/null 2>&1 || true

echo "$app_name installed at $app_target"
