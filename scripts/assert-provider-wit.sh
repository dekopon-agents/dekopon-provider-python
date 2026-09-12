#!/usr/bin/env bash
set -euo pipefail

json=${1:?usage: assert-provider-wit.sh <component-wit.json>}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
grep -Fxq 'package dekopon:provider@0.3.0;' "$root/wit/provider.wit" || {
  echo "error: mirrored provider package/version drifted" >&2
  exit 1
}
printf 'eac383801715cc62f41f7267de5c191827cfd2c45c766cda5600cfef2e1c03dd  %s\n' \
  "$root/wit/provider.wit" | sha256sum_check - >/dev/null
jq -e '
  (.worlds | length) == 1 and
  (.worlds[0].name == "root") and
  (.worlds[0].imports == {}) and
  ((.worlds[0].exports | keys | sort) == ["describe", "invoke"]) and
  (.worlds[0].exports.describe.function.params == []) and
  (.worlds[0].exports.describe.function.result == "string") and
  (.worlds[0].exports.invoke.function.params == [
    {"name":"capability","type":"string"},
    {"name":"input-json","type":"string"}
  ]) and
  (.worlds[0].exports.invoke.function.result == "string") and
  (.interfaces == [])
' "$json" >/dev/null || {
  echo "error: component WIT is not the import-free dekopon:provider@0.3.0 base shape" >&2
  exit 1
}
