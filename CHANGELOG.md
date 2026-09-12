# Changelog

## 0.2.0 - Unreleased

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
