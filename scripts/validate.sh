#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
cd "$root"

test -z "$(git ls-files '*.wasm')"
cargo +1.97.0 fmt --all -- --check
cargo +1.97.0 clippy --locked --all-targets -- -D warnings
cargo +1.97.0 test --locked --all-targets
cargo +1.93.0 check --locked --all-targets
cargo +1.97.0 check --locked --target wasm32-unknown-unknown
./scripts/assert-lock-and-feature-graph.sh Cargo.lock
cargo deny check licenses advisories bans sources
./scripts/check-third-party-notices.sh Cargo.lock THIRD_PARTY_NOTICES.md
bash -n scripts/*.sh
python3 - <<'PY'
import ast, pathlib
for path in pathlib.Path("scripts").glob("*.py"):
    ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
PY
ruby -e 'require "yaml"; ARGV.each { |f| YAML.safe_load(File.read(f), aliases: true) }' \
  .github/workflows/*.yml
if command -v actionlint >/dev/null 2>&1; then
  actionlint -color
fi
./scripts/build-component.sh
wasm-tools validate python-provider.wasm
wasm-tools component wit -j python-provider.wasm >/tmp/python-provider-wit.json
./scripts/assert-provider-wit.sh /tmp/python-provider-wit.json
if command -v wasmtime >/dev/null 2>&1; then
  ./scripts/test-wasmtime-smoke.sh python-provider.wasm
fi
./scripts/test-component-protocol.sh python-provider.wasm
./scripts/test-supported-and-denied-modules.sh python-provider.wasm
./scripts/test-yaml-policy.sh python-provider.wasm
./scripts/test-immediate-resource-limits.sh python-provider.wasm
./scripts/test-broker-testkit.sh python-provider.wasm
./scripts/measure-final-artifact.sh python-provider.wasm
sha256sum_check python-provider.wasm.sha256
./scripts/validate-workflows-and-release-layout.sh
