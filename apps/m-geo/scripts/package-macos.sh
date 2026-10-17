#!/bin/bash
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(node -p "require(process.argv[1]).version" "$app_root/package.json")"
arch="$(uname -m)"
output="${1:-$app_root/../../artifacts/m-geo-$version}"
bundle="$app_root/src-tauri/target/release/bundle/macos/M- GEO.app"
test -d "$bundle"
mkdir -p "$output"
stage="$(mktemp -d "${TMPDIR:-/tmp}/mgeo-package.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
ditto "$bundle" "$stage/M- GEO.app"
ln -s /Applications "$stage/Applications"
cp "$app_root/README.md" "$stage/使用说明.md"
cp "$app_root/src-tauri/resources/LICENSE.txt" "$stage/LICENSE.txt"
# Deterministic source folder; no Finder / AppleScript cosmetic layout dependency.
hdiutil create -volname "M- GEO $version Internal" -srcfolder "$stage" -ov -format UDZO "$output/M-GEO-$version-mac-$arch-internal.dmg"
hdiutil verify "$output/M-GEO-$version-mac-$arch-internal.dmg"
ditto "$bundle" "$output/M- GEO.app"
