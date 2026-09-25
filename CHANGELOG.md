# Changelog

## [Unreleased]

## [0.6.2] - 2026-09-25

### Fixed

- Send a fixed `User-Agent: dekopon-provider-python/<version>` on every `dekopon_requests` call.
  crates.io answered 403 to every request because none was sent. Scripts still set no headers.

## [0.6.1] - 2026-09-21

### Fixed

- Compile the component once per integration-test binary using a shared, process-local testkit
  cache. Coordinate broker loading to avoid concurrent cache publishers while preserving parallel
  invocations, independent host limits, and all sandbox and HTTP-grant coverage.

## [0.6.0] - 2026-09-20

### Changed

- Move to SDK 0.18.0 and HTTP 1.1.0; Python commands and buffered GET/HEAD behavior are unchanged.

## [0.5.0]

### Added

- Update real-host test dependency rustls to 0.23.45 for RUSTSEC-2026-0285.
- Ship one full HTTP `python-provider.wasm`: default-on `http` feature, `python.eval`, and
  `python` command, with native bounded `dekopon_requests` GET/HEAD and real-host grants.
  Keep exact import/WIT validation as provider-owned tests under the shared CI/release workflows;
  remove runtime variants. Pure scripts need no HTTP grant; empty-linker instantiation is refused.
- Merge #6's reusable workflow migration and RustPython VM build-environment patch. CI and release
  call provider-workflows v3 by immutable SHA; no local build/release/source-bundle pipeline remains.
- Move to `dekopon-provider-sdk` 0.15.0 and `dekopon-provider-sdk-testkit` 0.15.0.
  `CommandInvocation` gains `secret_use`, always `None` here: this provider proposes no secret use.
- Bare `python <<'EOF'` with something piped now proposes `python.eval` exactly like `python -`,
  matching CPython's own read of a non-tty stdin when given no file: `{"script": <piped value>}`.
  Bare argv with nothing piped, or an empty pipe, is still the same usage error as before.
- Delete the stale one-shot `release-recovery.yml` workflow left over from the v0.1.0 recovery;
  `ci.yml` and `release.yml` are unaffected.

## 0.3.0 - Unreleased

- Add the `python` command word, exported through `run-command` from
  `dekopon:provider/provider-cli@0.3.0`. `python -c CODE` and `python - <<'EOF'` propose
  `python.eval` with exactly `{"script": ...}`, authorized like a direct call; `python --help` and
  `--version` render at status 0 and usage errors at status 2, in the guest and without a VM.
  `python -` with nothing piped is declined as a usage error. `invoke` and its input are unchanged.
- Dekopon is removing every agent-facing way to reach a provider except its command word, and will
  refuse to boot a provider that declares capabilities and no word, so 0.2.0 stops loading there.
  0.13.0 hosts already dispatch this word.
- clap 4.6.6, through the SDK's `clap` feature, joins the component graph and the
  corresponding-source archive (MIT OR Apache-2.0). The provider now generates its own
  `wit-bindgen` 0.62.0 bindings for the `provider-cli` world of the unchanged, byte-pinned
  `wit/provider.wit`.

## 0.2.0 - 2026-09-12

- Move to `dekopon-provider-sdk` 0.13.0 and `dekopon:provider@0.3.0`. The retired `idempotency`
  capability classification is gone from the manifest, so an 0.11-era host will not load this
  component and an 0.13 host will not load the 0.1.0 one.
- Build on Rust 1.98.1 with wasm-tools 1.259.0 and verify against Wasmtime 48.0.2.
- Retire `dekopon-run`, which stops at 0.11.1 and requires the removed manifest field. Protocol,
  sandbox, YAML-policy, and host-termination coverage now runs against the real broker host
  through `dekopon-provider-sdk-testkit`'s `FakeBroker` in `tests/broker.rs`.
- Derive the release version from `Cargo.toml` and the pushed tag throughout the release workflow
  and the asset, bundle, and OCI-manifest scripts, instead of pinning them to one shipped release.
- Delete the package-visibility flip from publication and rollback. It called a GHCR endpoint that
  does not exist, and only stayed dormant through 0.1.0 because both packages were created by that
  run. A version is now publicly resolvable between its push and its authentication; cleanup
  deleting this run's version by digest is what bounds a failed authentication.

## 0.1.0 - 2026-08-24

- Add the import-free `python.eval` capability with a fresh exact RustPython 0.5.0 VM per call.
- Guarantee bounded stdout and safe JSON-shaped results, structured script errors, `json`, `re`,
  and constrained native YAML.
- Add zero-import/resource/component host gates and a mechanically interlocked release workflow.
- Close Python introspection recovery of import/eval/compile callables, test the selected 64 MiB
  profile during guest allocation, and make OCI version publication run-owned and rollback-safe.
- Record the owner's acceptance of the exact LGPL Malachite/r-efi graph for this standalone
  provider while retaining MIT OR Apache-2.0 on original project source.
- Add verbatim LGPL/GPL texts, prominent notices, deterministic complete corresponding-source and
  CycloneDX SBOM generation, and a clean offline test that modifies Malachite and relinks a valid
  import-free component.
- Publish the deliberately expanded compliance asset set through the GitHub Release and a linked
  public version-only source OCI artifact while preserving the provider OCI as one Wasm layer.

The license-policy decision is complete. Publication still requires the repository variable,
immutable annotated tag, remote, and all transactional release gates in `RELEASE_COMPLIANCE.md`.
