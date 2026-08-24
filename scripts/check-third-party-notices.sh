#!/usr/bin/env bash
set -euo pipefail

lock=${1:-Cargo.lock}
notices=${2:-THIRD_PARTY_NOTICES.md}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
for file in "$lock" "$notices"; do
  [[ -f "$file" ]] || { echo "error: missing $file" >&2; exit 1; }
done

python3 - "$lock" <<'PY'
import pathlib, sys, tomllib
packages = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))["package"]
actual = {(p["name"], p["version"]): p.get("checksum") for p in packages}
required = {
    ("malachite-base", "0.9.2"): "a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e",
    ("malachite-bigint", "0.9.2"): "cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef",
    ("malachite-nz", "0.9.2"): "a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d",
    ("malachite-q", "0.9.2"): "5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94",
    ("r-efi", "5.3.0"): "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f",
    ("r-efi", "6.0.0"): "f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf",
}
for package, checksum in required.items():
    if actual.get(package) != checksum:
        raise SystemExit(f"error: locked LGPL package drift: {package}")
PY

for package in \
  'malachite-base 0.9.2' 'malachite-bigint 0.9.2' \
  'malachite-nz 0.9.2' 'malachite-q 0.9.2' \
  'r-efi 5.3.0' 'r-efi 6.0.0' \
  'rustpython-vm 0.5.0' 'rustpython-stdlib 0.5.0' 'rustpython-pylib 0.5.0' \
  'yaml-rust2 0.12.0' 'paste 1.0.15' 'unic-char-property'; do
  grep -Fiq "$package" "$notices" || {
    echo "error: notices do not name $package" >&2
    exit 1
  }
done
for term in \
  LGPL-3.0-only LGPL-2.1-or-later Python-2.0.1 PSF Unicode BSD \
  'combined WebAssembly component' 'corresponding-source' 'modify/replace Malachite' \
  'owner' 'does not claim attorney review' 'provider-python-source:0.1.0' 'MIT OR Apache-2.0'; do
  grep -Fiq "$term" "$notices" || {
    echo "error: notices do not cover $term" >&2
    exit 1
  }
done

echo '20e50fe7aae3e56378ebf0417d9de904f55a0e61e4df315333e632a4d3555d95  LICENSE-LGPL-2.1' |
  (cd "$root" && sha256sum_check -)
echo 'e3a994d82e644b03a792a930f574002658412f62407f5fee083f2555c5f23118  LICENSE-LGPL-3.0' |
  (cd "$root" && sha256sum_check -)
echo '3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986  LICENSE-GPL-3.0' |
  (cd "$root" && sha256sum_check -)
