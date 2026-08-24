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
version=0.1.0
archive=$(release_source_archive "$version")
sbom=$(release_sbom "$version")
reference=${1:-}
temporary=$(mktemp -d "${TMPDIR:-/tmp}/dekopon-python-source-repro.XXXXXX")
cleanup() {
  rm -rf "$temporary"
}
trap cleanup EXIT
mkdir -p "$temporary/a" "$temporary/b"
"$root/scripts/build-source-bundle.sh" "$temporary/a"
"$root/scripts/build-source-bundle.sh" "$temporary/b"
for name in "$archive" "$archive.sha256" "$sbom"; do
  cmp "$temporary/a/$name" "$temporary/b/$name"
done
if [[ -n "$reference" ]]; then
  reference=$(cd "$reference" && pwd -P)
  for name in "$archive" "$archive.sha256" "$sbom"; do
    cmp "$reference/$name" "$temporary/a/$name"
  done
fi
printf 'source archive and SBOM reproduced byte-for-byte: %s\n' \
  "$(sha256sum_digest "$temporary/a/$archive")"
