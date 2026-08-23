#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
component=${1:-"$root/python-provider.wasm"}
required_rust="1.97.0"
required_rustc="rustc 1.97.0 (2d8144b78 2026-07-07)"
required_wasm_tools="1.236.1"

[[ "$(rustup run "$required_rust" rustc --version)" == "$required_rustc" ]] || {
  echo "error: expected $required_rustc" >&2
  exit 1
}
[[ "$(wasm-tools --version)" == "wasm-tools $required_wasm_tools" ]] || {
  echo "error: expected wasm-tools $required_wasm_tools" >&2
  exit 1
}

# RustPython 0.5.0's build.rs freezes every visible build-time environment variable into
# `_sysconfigdata`. Build the distributable from a fixed-path, scrubbed standalone source snapshot:
# this prevents credential capture and gives two independent checkouts the same package/build paths.
# It does not replace the global rustc wrapper, CARGO_TARGET_DIR, or incremental policy; ordinary
# Cargo and the configured global sccache remain in force inside the standalone snapshot.
if [[ ${DEKOPON_PYTHON_CANONICAL_INNER:-0} != 1 ]]; then
  if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    test -z "$(git -C "$root" ls-files '*.wasm')" || {
      echo "error: generated Wasm must never be tracked" >&2
      exit 1
    }
    git -C "$root" diff --quiet
    git -C "$root" diff --cached --quiet
  fi

  canonical=${DEKOPON_PYTHON_CANONICAL_ROOT:-/tmp/dekopon-python-provider-canonical}
  lock="${canonical}.lock"
  if ! mkdir "$lock" 2>/dev/null; then
    echo "error: canonical provider build already active at $canonical" >&2
    exit 1
  fi
  cleanup() {
    rm -rf "$lock"
  }
  trap cleanup EXIT
  rm -rf "$canonical"
  mkdir -p "$canonical"

  if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$root" archive --format=tar HEAD | tar -xf - -C "$canonical"
  else
    (
      cd "$root"
      tar --exclude='./target' --exclude='./python-provider.wasm' \
        --exclude='./python-provider.wasm.sha256' -cf - .
    ) | tar -xf - -C "$canonical"
  fi

  (
    cd "$canonical"
    DEKOPON_PYTHON_CANONICAL_INNER=1 \
      ./scripts/build-component.sh "$canonical/python-provider.wasm"
  )

  # Leave the exact sanitized release build available at the validation command's conventional
  # target path. Remove only this generated release subtree, never another worktree or shared cache.
  release_parent="$root/target/wasm32-unknown-unknown"
  rm -rf "$release_parent/release"
  mkdir -p "$release_parent"
  cp -R "$canonical/target/wasm32-unknown-unknown/release" "$release_parent/release"

  mkdir -p "$(dirname "$component")"
  cp "$canonical/python-provider.wasm" "$component"
  (
    cd "$(dirname "$component")"
    sha256sum "$(basename "$component")" >"$(basename "$component").sha256"
  )
  wasm-tools validate "$component"
  "$root/scripts/assert-zero-core-imports.sh" "$component"
  sha256sum --check --strict "${component}.sha256"

  for forbidden in GH_TOKEN GITHUB_TOKEN GH_PAT OF_PASSWORD UNIFI_SSH_PASSWORD \
    AWS_SECRET_ACCESS_KEY PI_SESSION_ID; do
    if LC_ALL=C grep -aF -- "$forbidden" "$component" >/dev/null; then
      echo "error: sanitized component contains forbidden build-environment key $forbidden" >&2
      exit 1
    fi
  done
  printf 'generated %s (%s bytes) from scrubbed canonical source\n' \
    "$component" "$(wc -c <"$component" | tr -d ' ')"
  exit 0
fi

core="$root/target/wasm32-unknown-unknown/release/dekopon_python_provider.wasm"

cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
cargo_home=$(cd "$cargo_home" && pwd -P)
sysroot=$(rustup run "$required_rust" rustc --print sysroot)
sysroot=$(cd "$sysroot" && pwd -P)
rustflags=(
  '--cfg=getrandom_backend="custom"'
  '--check-cfg=cfg(getrandom_backend, values("custom"))'
  "--remap-path-prefix=$root=/dekopon/source"
  "--remap-path-prefix=$cargo_home=/dekopon/cargo"
  "--remap-path-prefix=$sysroot=/dekopon/rust/$required_rust"
)
encoded_rustflags=$(printf '%s\x1f' "${rustflags[@]}")
encoded_rustflags=${encoded_rustflags%$'\x1f'}

safe_path="$cargo_home/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
env -i \
  HOME="$HOME" \
  USER="$(id -un)" \
  LOGNAME="$(id -un)" \
  PATH="$safe_path" \
  CARGO_HOME="$cargo_home" \
  RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
  CARGO_TERM_COLOR=never \
  CARGO_ENCODED_RUSTFLAGS="$encoded_rustflags" \
  LANG=C.UTF-8 \
  LC_ALL=C \
  SOURCE_DATE_EPOCH=0 \
  TMPDIR=/tmp \
  cargo +"$required_rust" rustc \
    --jobs 1 \
    --locked \
    --manifest-path "$root/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release \
    -- \
    -C metadata=dekopon-python-provider-0.1.0-repro-v1 \
    -C extra-filename=

wasm-tools validate "$core"
"$root/scripts/assert-zero-core-imports.sh" "$core"
wasm-tools component new "$core" -o "$component"
wasm-tools validate "$component"
"$root/scripts/assert-zero-core-imports.sh" "$component"
(
  cd "$(dirname "$component")"
  sha256sum "$(basename "$component")" >"$(basename "$component").sha256"
)
sha256sum --check --strict "${component}.sha256"
