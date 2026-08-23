# Changelog

## 0.1.0 - Unreleased

- Add the import-free `python.eval` capability with a fresh exact RustPython 0.5.0 VM per call.
- Guarantee bounded stdout and safe JSON-shaped results, structured script errors, `json`, `re`,
  and constrained native YAML.
- Add zero-import/resource/component host gates and a publication-disabled release workflow.
- Close Python introspection recovery of import/eval/compile callables, test the selected 64 MiB
  profile during guest allocation, and make OCI version publication run-owned and rollback-safe.

Publication remains held pending the LGPL/Malachite policy decision documented in
`RELEASE_COMPLIANCE.md`.
