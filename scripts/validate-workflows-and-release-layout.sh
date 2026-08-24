#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-release-assets.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-release-assets.sh"
test -z "$(git -C "$root" ls-files '*.wasm' '*.wasm.sha256' '*.tar.gz' '*.cdx.json' 'vendor/**')" || {
  echo 'error: generated binary/compliance output is tracked' >&2
  exit 1
}
for workflow in "$root/.github/workflows/ci.yml" "$root/.github/workflows/release.yml"; do
  [[ -f "$workflow" ]] || { echo "error: missing $workflow" >&2; exit 1; }
  if grep -E 'uses: [^ ]+@(main|master|v[0-9]+([.][0-9]+)*)[[:space:]]*$' "$workflow"; then
    echo "error: action is not pinned by full commit SHA in $workflow" >&2
    exit 1
  fi
  while IFS= read -r ref; do
    [[ "$ref" =~ ^[0-9a-f]{40}$ ]] || {
      echo "error: action ref is not a 40-character SHA: $ref" >&2
      exit 1
    }
  done < <(sed -nE 's/^[[:space:]]*uses: [^@]+@([0-9a-f]+).*/\1/p' "$workflow")
done

release="$root/.github/workflows/release.yml"
for required in \
  'v0.1.0' \
  'PROVIDER_PYTHON_RELEASE_APPROVED' \
  'ghcr.io/dekopon-agents/provider-python' \
  'ghcr.io/dekopon-agents/provider-python-source' \
  'OCI_STAGING_REPOSITORY' \
  'SOURCE_OCI_STAGING_REPOSITORY' \
  'application/vnd.dekopon.provider.v1+wasm' \
  'application/vnd.dekopon.provider.source.v1' \
  'python-provider.wasm:application/wasm' \
  'org.dekopon.corresponding-source.oci' \
  'org.dekopon.corresponding-source.digest' \
  'org.dekopon.corresponding-source.archive' \
  'org.opencontainers.image.licenses=LGPL-3.0-only' \
  'test-source-bundle-reproducibility.sh' \
  'test-source-bundle-relink.sh' \
  'verify-release-assets.sh' \
  'verify-oci-manifest.py' \
  'docker logout ghcr.io' \
  'env -u GH_TOKEN -u GITHUB_TOKEN' \
  'manifest delete --force' \
  'provider_initial_visibility' \
  'source_initial_visibility' \
  'ensure_existing_staging_private' \
  'cleanup_owned_staging_ref' \
  'record_owned_manifest "$OCI_STAGING_REPOSITORY" "$provider_stage"' \
  'remove only this run' \
  'always() && (failure() || cancelled())'; do
  grep -Fq "$required" "$release" || {
    echo "error: release workflow lacks $required" >&2
    exit 1
  }
done
[[ "$(grep -Fc 'org.opencontainers.image.licenses=LGPL-3.0-only' "$release")" -eq 2 ]] || {
  echo 'error: both OCI manifests must carry the exact accepted SPDX license annotation' >&2
  exit 1
}
grep -Fq './scripts/prepare-release-assets.sh 0.1.0 dist' \
  "$root/.github/workflows/ci.yml" || {
  echo 'error: regular CI does not prepare the exact release asset set' >&2
  exit 1
}
python3 - "$release" <<'PY'
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
markers = [
    "# Corresponding source is the first final effect",
    "/provider-python-source/visibility",
    '>"$RUNNER_TEMP/anonymous-source-before-provider.json"',
    '"$OCI_STAGING_REPOSITORY@$PROVIDER_DIGEST" "$provider_ref"',
    "/provider-python/visibility",
]
positions = []
for marker in markers:
    if text.count(marker) != 1:
        raise SystemExit(f"error: release transaction marker drifted: {marker}")
    positions.append(text.index(marker))
if positions != sorted(positions):
    raise SystemExit("error: provider publication can precede anonymous corresponding-source verification")
PY

asset_count=$(release_asset_names 0.1.0 | wc -l | tr -d ' ')
source_count=$(source_oci_asset_names 0.1.0 | wc -l | tr -d ' ')
[[ "$asset_count" == 14 && "$source_count" == 13 ]] || {
  echo 'error: immutable release/source asset counts drifted' >&2
  exit 1
}
while IFS= read -r asset; do
  grep -Fq "$asset" "$release" || {
    echo "error: release workflow omits immutable asset $asset" >&2
    exit 1
  }
done < <(release_asset_names 0.1.0)
[[ "$(grep -Fc 'python-provider.wasm:application/wasm' "$release")" -eq 1 ]] || {
  echo 'error: provider OCI must declare exactly one application/wasm layer' >&2
  exit 1
}
# Intentional literal workflow assertion.
# shellcheck disable=SC2016
grep -Fq 'test "$(find dist -maxdepth 1 -type f | wc -l)" -eq 14' "$release" || {
  echo 'error: release workflow does not enforce the expanded 14-asset layout' >&2
  exit 1
}
if grep -E 'provider-python(-source)?:(latest|stable)|--tag[ =]+(latest|stable)' "$release"; then
  echo 'error: release workflow must never publish a mutable latest/stable tag' >&2
  exit 1
fi
for document in README.md RELEASE_COMPLIANCE.md THIRD_PARTY_NOTICES.md RELINKING.md SECURITY.md; do
  grep -Eiq 'corresponding[- ]source' "$root/$document" || {
    echo "error: $document lacks a corresponding-source notice" >&2
    exit 1
  }
done
