# Release compliance gate

Publication is disabled by default. Before setting the GitHub repository variable
`PROVIDER_PYTHON_RELEASE_APPROVED=true`, the owner/legal reviewer must record all of the following:

- an approved LGPL-3.0-only corresponding-source and relinkability design for the four statically
  linked Malachite 0.9.2 packages, or a reviewed replacement/fork removing that graph;
- whether the approved design permits exactly the two technical GitHub Release assets
  `python-provider.wasm` and `python-provider.wasm.sha256`; if not, update the workflow and layout
  assertions deliberately;
- regenerated SBOM/notices and reviewed cargo-deny exceptions;
- green main CI, reproducible bytes, final host/resource measurements, and trusted source/tag;
- an annotated `v0.1.0` tag contained in `main`.

The variable is a mechanical interlock, not standing publication permission. Never publish
`latest`; the only OCI tag for this release is `0.1.0`.
