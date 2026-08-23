#!/usr/bin/env bash
set -euo pipefail

json=${1:?usage: assert-provider-wit.sh <component-wit.json>}
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
