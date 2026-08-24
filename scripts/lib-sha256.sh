#!/usr/bin/env bash
# shellcheck shell=bash

sha256sum_select() {
  local candidate
  for candidate in sha256sum gsha256sum; do
    if command -v "$candidate" >/dev/null 2>&1; then
      command -v "$candidate"
      return 0
    fi
  done
  echo 'error: SHA-256 verifier unavailable; install GNU coreutils (sha256sum/gsha256sum)' >&2
  return 1
}

sha256sum_run() {
  local executable
  executable=$(sha256sum_select)
  "$executable" "$@"
}

sha256sum_check() {
  sha256sum_run --check --strict "$@"
}

sha256sum_digest() {
  local file=${1:?usage: sha256sum_digest FILE}
  sha256sum_run "$file" | awk '{print $1}'
}
