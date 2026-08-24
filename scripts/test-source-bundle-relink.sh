#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-release-assets.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-release-assets.sh"
# shellcheck source=lib-sha256.sh
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
version=${VERSION:-0.1.0}
archive=${1:-"$root/dist/$(release_source_archive "$version")"}
sbom=${2:-"$root/dist/$(release_sbom "$version")"}
checksum="$archive.sha256"
for file in "$archive" "$checksum" "$sbom"; do
  [[ -f "$file" ]] || { echo "error: missing source-bundle test input $file" >&2; exit 1; }
done
archive=$(cd "$(dirname "$archive")" && pwd -P)/$(basename "$archive")
sbom=$(cd "$(dirname "$sbom")" && pwd -P)/$(basename "$sbom")
(cd "$(dirname "$archive")" && sha256sum_check "$(basename "$checksum")")

temporary=$(mktemp -d "${TMPDIR:-/tmp}/dekopon-python-relink.XXXXXX")
cleanup() {
  rm -rf "$temporary"
}
trap cleanup EXIT
tar -xzf "$archive" -C "$temporary"
source_root="$temporary/dekopon-python-provider-$version"
[[ -d "$source_root" ]]
cmp "$source_root/$(basename "$sbom")" "$sbom"
python3 "$source_root/scripts/source-file-manifest.py" verify "$source_root"
python3 "$source_root/scripts/verify-vendored-source.py" "$source_root"
[[ "$(jq -er .version "$source_root/SOURCE_MANIFEST.json")" == "$version" ]]
[[ "$(jq -er .gitRevision "$source_root/SOURCE_MANIFEST.json")" == "$(git -C "$root" rev-parse HEAD)" ]]
grep -Fq 'offline = true' "$source_root/.cargo/config.toml"
(
  cd "$source_root"
  cargo +1.97.0 metadata --locked --offline --format-version 1 >/dev/null
)

# Exercise the LGPL relinking path, rather than merely rebuilding pristine bytes. A recipient makes
# a harmless source edit in the exact Malachite graph and refreshes Cargo's vendored file hash.
modified='vendor/malachite-base-0.9.2/src/lib.rs'
printf '\n// recipient offline relink validation: harmless source modification\n' >>"$source_root/$modified"
python3 "$source_root/scripts/refresh-vendored-checksum.py" \
  "$source_root/vendor/malachite-base-0.9.2" src/lib.rs
grep -Fq 'recipient offline relink validation' "$source_root/$modified"
python3 "$source_root/scripts/verify-vendored-source.py" "$source_root"

build_log="$temporary/offline-relink.log"
(
  cd "$source_root"
  DEKOPON_PYTHON_CANONICAL_ROOT="$temporary/canonical" ./scripts/build-component.sh
) >"$build_log" 2>&1 || {
  cat "$build_log" >&2
  exit 1
}
if grep -E 'Updating crates.io|Downloading crates|Downloaded ' "$build_log"; then
  echo 'error: offline relink unexpectedly accessed a dependency registry' >&2
  exit 1
fi
grep -Fq 'Compiling malachite-base v0.9.2' "$build_log" || {
  echo 'error: relink log does not prove the modified Malachite crate was rebuilt' >&2
  cat "$build_log" >&2
  exit 1
}
wasm-tools validate "$source_root/python-provider.wasm"
"$source_root/scripts/assert-zero-core-imports.sh" "$source_root/python-provider.wasm"
(cd "$source_root" && sha256sum_check python-provider.wasm.sha256)
printf 'offline relink succeeded after modifying %s; component SHA-256 %s\n' \
  "$modified" "$(sha256sum_digest "$source_root/python-provider.wasm")"
