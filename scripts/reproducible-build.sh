#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
destination=/tmp/dekopon-python-repro
[[ -z "$(git -C "$root" status --porcelain --untracked-files=no)" ]] || {
  echo "error: reproducibility check requires no tracked-file modifications" >&2
  exit 1
}
rm -rf "$destination"
mkdir -p "$destination"
git -C "$root" archive --format=tar HEAD | tar -xf - -C "$destination"
# Force an independent compile while retaining the ordinary per-source default target policy. The
# canonical source path stays identical; only this inactive standalone snapshot target is removed.
canonical=${DEKOPON_PYTHON_CANONICAL_ROOT:-/tmp/dekopon-python-provider-canonical}
rm -rf "$canonical/target"
(
  cd "$destination"
  ./scripts/build-component.sh "$destination/python-provider.wasm"
)
cmp "$root/python-provider.wasm" "$destination/python-provider.wasm"
cmp "$root/python-provider.wasm.sha256" "$destination/python-provider.wasm.sha256"
