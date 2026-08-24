#!/usr/bin/env bash
set -euo pipefail

json=${1:?usage: assert-provider-wit.sh <component-wit.json>}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
grep -Fxq 'package dekopon:provider@0.2.0;' "$root/wit/provider.wit" || {
  echo "error: mirrored provider package/version drifted" >&2
  exit 1
}
printf '02ba5a92067f53bc8f48e10bf221229c5b7f33f791a031741da5011c32ab37c9  %s\n' \
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
  echo "error: component WIT is not the import-free dekopon:provider@0.2.0 base shape" >&2
  exit 1
}
