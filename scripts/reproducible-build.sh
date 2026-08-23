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
(
  cd "$destination"
  ./scripts/build-component.sh "$destination/python-provider.wasm"
)
cmp "$root/python-provider.wasm" "$destination/python-provider.wasm"
cmp "$root/python-provider.wasm.sha256" "$destination/python-provider.wasm.sha256"
