#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
test -z "$(git -C "$root" ls-files '*.wasm')" || {
  echo "error: tracked Wasm found" >&2
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
  'python-provider.wasm' \
  'python-provider.wasm.sha256' \
  'ghcr.io/dekopon-agents/provider-python' \
  'application/vnd.dekopon.provider.v1+wasm' \
  'application/wasm' \
  'PROVIDER_PYTHON_RELEASE_APPROVED'; do
  grep -Fq "$required" "$release" || { echo "error: release workflow lacks $required" >&2; exit 1; }
done
if grep -E 'provider-python:(latest|stable)|--tag[ =]+latest' "$release"; then
  echo "error: release workflow must never publish latest" >&2
  exit 1
fi
