#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-release-assets.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-release-assets.sh"
version=${1:-0.1.0}
out=${2:-"$root/dist"}
release_version_is_valid "$version" || {
  echo "error: only immutable release version 0.1.0 is supported" >&2
  exit 1
}
actual_version=$(cargo metadata --locked --no-deps --format-version 1 |
  jq -er '.packages[] | select(.name == "dekopon-python-provider") | .version')
[[ "$actual_version" == "$version" ]] || {
  echo "error: Cargo package version $actual_version does not match $version" >&2
  exit 1
}
[[ -f "$root/python-provider.wasm" && -f "$root/python-provider.wasm.sha256" ]] || {
  echo 'error: build python-provider.wasm before preparing release assets' >&2
  exit 1
}
(cd "$root" && sha256sum --check --strict python-provider.wasm.sha256)

rm -rf "$out"
mkdir -p "$out"
cp "$root/python-provider.wasm" "$root/python-provider.wasm.sha256" "$out/"
"$root/scripts/build-source-bundle.sh" "$out"
cp \
  "$root/THIRD_PARTY_NOTICES.md" \
  "$root/RELEASE_COMPLIANCE.md" \
  "$root/RELINKING.md" \
  "$root/LICENSE-MIT" \
  "$root/LICENSE-APACHE" \
  "$root/LICENSE-LGPL-2.1" \
  "$root/LICENSE-LGPL-3.0" \
  "$root/LICENSE-GPL-3.0" \
  "$out/"

(
  cd "$out"
  : >SHA256SUMS
  while IFS= read -r name; do
    [[ "$name" == SHA256SUMS ]] && continue
    sha256sum "$name" >>SHA256SUMS
  done < <(release_asset_names "$version")
)
"$root/scripts/verify-release-assets.sh" "$out" "$version"
printf 'prepared %s immutable release assets in %s\n' \
  "$(release_asset_names "$version" | wc -l | tr -d ' ')" "$out"
