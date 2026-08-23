#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
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
sha256sum --check --strict python-provider.wasm.sha256
./scripts/validate-workflows-and-release-layout.sh
