#!/usr/bin/env bash
# shellcheck shell=bash

release_version_is_valid() {
  [[ ${1:-} == 0.1.0 ]]
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
