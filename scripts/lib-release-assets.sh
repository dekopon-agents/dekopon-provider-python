#!/usr/bin/env bash
# shellcheck shell=bash

# The release version is whatever Cargo.toml declares; a tag, an asset name, and an OCI tag are
# all derived from it rather than pinned to one shipped release.
release_package_version() {
  python3 - "${1:-$root}/Cargo.toml" <<'PYTHON'
import pathlib, sys, tomllib
package = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))["package"]
if package["name"] != "dekopon-python-provider":
    raise SystemExit(f"error: unexpected package {package['name']}")
print(package["version"])
PYTHON
}

release_version_is_valid() {
  [[ ${1:-} =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

release_source_archive() {
  printf 'dekopon-python-provider-%s-relink-source.tar.gz\n' "$1"
}

release_sbom() {
  printf 'dekopon-python-provider-%s.cdx.json\n' "$1"
}

release_asset_names() {
  local version=$1 archive sbom
  archive=$(release_source_archive "$version")
  sbom=$(release_sbom "$version")
  cat <<EOF
python-provider.wasm
python-provider.wasm.sha256
$archive
$archive.sha256
$sbom
THIRD_PARTY_NOTICES.md
RELEASE_COMPLIANCE.md
RELINKING.md
LICENSE-MIT
LICENSE-APACHE
LICENSE-LGPL-2.1
LICENSE-LGPL-3.0
LICENSE-GPL-3.0
SHA256SUMS
EOF
}

source_oci_asset_names() {
  release_asset_names "$1" | grep -Fvx 'python-provider.wasm'
}
