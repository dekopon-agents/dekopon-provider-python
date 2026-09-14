#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
variant=${1:-offline}
case "$variant" in
  offline) artifact=python-provider; release_tree=target/wasm32-unknown-unknown/release ;;
  http) artifact=python-http-provider; release_tree=target/http-component/wasm32-unknown-unknown/release ;;
  *) echo "error: variant must be offline or http" >&2; exit 1 ;;
esac
destination=/tmp/dekopon-python-repro
diagnostics=$(mktemp -d "${TMPDIR:-/tmp}/dekopon-python-repro-inputs.XXXXXX")
cleanup() {
  rm -rf "$destination" "$diagnostics"
}
trap cleanup EXIT
[[ -z "$(git -C "$root" status --porcelain --untracked-files=no)" ]] || {
  echo "error: reproducibility check requires no tracked-file modifications" >&2
  exit 1
}
rm -rf "$destination"
mkdir -p "$destination"
git -C "$root" archive --format=tar HEAD | tar -xf - -C "$destination"
env_a=$(find \
  "$root/$release_tree/build" \
  -path '*/rustpython-vm-*/out/env_vars.rs' -type f -print)
[[ -f "$env_a" ]] || {
  echo 'error: build the canonical component before checking reproducibility' >&2
  exit 1
}
cp "$env_a" "$diagnostics/rustpython-env-a.rs"
cp "$root/$release_tree/dekopon_python_provider.wasm" \
  "$diagnostics/core-a.wasm"

# Force an independent compile while retaining the ordinary per-source default target policy. The
# canonical source path stays identical; only this inactive standalone snapshot target is removed.
canonical=${DEKOPON_PYTHON_CANONICAL_ROOT:-/tmp/dekopon-python-provider-canonical}
if [[ "$variant" == http ]]; then canonical="${canonical}-http"; fi
rm -rf "$canonical/target"
(
  cd "$destination"
  ./scripts/build-component.sh "$destination/$artifact.wasm" "$variant"
)
env_b=$(find \
  "$destination/$release_tree/build" \
  -path '*/rustpython-vm-*/out/env_vars.rs' -type f -print)
test -f "$env_b"
cp "$env_b" "$diagnostics/rustpython-env-b.rs"
cmp "$diagnostics/rustpython-env-a.rs" "$diagnostics/rustpython-env-b.rs"
cmp "$diagnostics/core-a.wasm" \
  "$destination/$release_tree/dekopon_python_provider.wasm"
cmp "$root/$artifact.wasm" "$destination/$artifact.wasm"
cmp "$root/$artifact.wasm.sha256" "$destination/$artifact.wasm.sha256"
printf 'reproduced RustPython environment %s, core %s, and component %s\n' \
  "$(sha256sum_digest "$diagnostics/rustpython-env-a.rs")" \
  "$(sha256sum_digest "$diagnostics/core-a.wasm")" \
  "$(sha256sum_digest "$root/$artifact.wasm")"
