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
delivery_stage="$(mktemp -d "$output/.mgeo-delivery.XXXXXX")"
cleanup() {
  if [[ -d "$stage/previous-output.app" && ! -e "$output/M- GEO.app" ]]; then
    mv "$stage/previous-output.app" "$output/M- GEO.app"
  fi
  rm -rf "$stage" "$delivery_stage"
}
trap cleanup EXIT
ditto "$bundle" "$stage/M- GEO.app"
packaged_app="$stage/M- GEO.app"
# Rust's linker signature covers only the executable; seal the whole local bundle.
# Preserve a valid developer signature, and never silently replace a broken one.
if ! codesign --verify --deep --strict "$packaged_app"; then
  signature="$(codesign -dv "$packaged_app" 2>&1 || true)"
  if [[ "$signature" == *"Signature=adhoc"* || "$signature" == *"code object is not signed at all"* ]]; then
    codesign --force --sign - --timestamp=none "$packaged_app"
  else
    echo "Refusing to replace an invalid non-ad-hoc signature." >&2
    exit 1
  fi
fi
codesign --verify --deep --strict "$packaged_app"
ln -s /Applications "$stage/Applications"
cp "$app_root/README.md" "$stage/使用说明.md"
cp "$app_root/src-tauri/resources/LICENSE.txt" "$stage/LICENSE.txt"
# Deterministic source folder; no Finder / AppleScript cosmetic layout dependency.
hdiutil create -volname "M- GEO $version Internal" -srcfolder "$stage" -ov -format UDZO "$output/M-GEO-$version-mac-$arch-internal.dmg"
hdiutil verify "$output/M-GEO-$version-mac-$arch-internal.dmg"
# ditto merges existing folders; use a fresh sibling to avoid obsolete resources.
ditto "$packaged_app" "$delivery_stage/M- GEO.app"
codesign --verify --deep --strict "$delivery_stage/M- GEO.app"
if [[ -e "$output/M- GEO.app" ]]; then
  mv "$output/M- GEO.app" "$stage/previous-output.app"
fi
mv "$delivery_stage/M- GEO.app" "$output/M- GEO.app"
codesign --verify --deep --strict "$output/M- GEO.app"
