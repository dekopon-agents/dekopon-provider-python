# Changelog

## 0.1.0 - Unreleased

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

The v0.1.0 license-policy decision is complete. Publication still requires the repository variable,
immutable annotated tag, remote, and all transactional release gates in `RELEASE_COMPLIANCE.md`.
