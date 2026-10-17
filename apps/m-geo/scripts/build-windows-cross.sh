#!/usr/bin/env bash
# NSIS cross-build only. A successful build does not prove Windows runtime behavior.
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$app_root/../.." && pwd)"
cd "$app_root"
version="$(node -p "JSON.parse(require('fs').readFileSync('package.json','utf8')).version")"
output="${1:-$repo_root/artifacts/m-geo-$version}"
target=x86_64-pc-windows-msvc
for tool in pnpm cargo rustup cargo-xwin clang-cl lld-link llvm-rc makensis python3; do
  command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
rustup target list --installed | grep -qx "$target" || {
  echo "Install the Rust target first: rustup target add $target" >&2; exit 1;
}
mkdir -p "$output"
# Tauri's own beforeBuildCommand runs the typechecked frontend production build.
pnpm tauri build --ci --runner cargo-xwin --target "$target" --bundles nsis \
  --config '{"bundle":{"useLocalToolsDir":true}}' -- --locked
source_bundle="${CARGO_TARGET_DIR:-$app_root/src-tauri/target}/$target/release/bundle/nsis"
shopt -s nullglob
installers=("$source_bundle"/*"_${version}_"*-setup.exe)
if [[ ${#installers[@]} -ne 1 ]]; then
  echo "Expected exactly one NSIS installer for $version in $source_bundle" >&2; exit 1
fi
destination="$output/M-GEO-$version-windows-x64-internal-setup.exe"
cp "${installers[0]}" "$destination"
python3 "$app_root/scripts/verify-windows-package.py" \
  --app "${CARGO_TARGET_DIR:-$app_root/src-tauri/target}/$target/release/m-geo.exe" \
  --installer "$destination" --icon "$app_root/src-tauri/icons/icon.ico" --version "$version" \
  > "$output/windows-pe-verification.json"
shasum -a 256 "$destination"
echo "Built unsigned Windows x64 installer. Windows installation/runtime remain unverified."
