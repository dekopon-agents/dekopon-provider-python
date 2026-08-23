#!/usr/bin/env bash
set -euo pipefail

lock=${1:-Cargo.lock}
notices=${2:-THIRD_PARTY_NOTICES.md}
for file in "$lock" "$notices"; do
  [[ -f "$file" ]] || { echo "error: missing $file" >&2; exit 1; }
done

for package in \
  'malachite-base' 'malachite-bigint' 'malachite-nz' 'malachite-q' \
  'rustpython-vm' 'rustpython-stdlib' 'rustpython-pylib' \
  'yaml-rust2' 'paste' 'unic-char-property'; do
  grep -Fq "name = \"$package\"" "$lock" || {
    echo "error: expected locked package $package is absent" >&2
    exit 1
  }
  grep -Fiq "$package" "$notices" || {
    echo "error: notices do not name $package" >&2
    exit 1
  }
done
for term in LGPL-3.0-only LGPL-2.1-or-later Python-2.0.1 PSF Unicode BSD publication; do
  grep -Fqi "$term" "$notices" || {
    echo "error: notices do not cover $term" >&2
    exit 1
  }
done
