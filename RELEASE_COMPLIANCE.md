# v0.1.0 release compliance design

## Recorded policy decision

The repository owner has accepted the exact LGPL dependency constraint and this distribution /
corresponding-source / relinkability design for the standalone optional
`dekopon-agents/dekopon-provider-python` v0.1.0 release. The accepted narrow packages are:

| Package | Version | License | crates.io archive SHA-256 |
|---|---:|---|---|
| `malachite-base` | 0.9.2 | LGPL-3.0-only | `a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e` |
| `malachite-bigint` | 0.9.2 | LGPL-3.0-only | `cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef` |
| `malachite-nz` | 0.9.2 | LGPL-3.0-only | `a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d` |
| `malachite-q` | 0.9.2 | LGPL-3.0-only | `5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94` |
| `r-efi` | 5.3.0 | LGPL-2.1-or-later | `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f` |
| `r-efi` | 6.0.0 | LGPL-2.1-or-later | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` |

The four Malachite crates are embedded in the static Wasm graph. The `r-efi` crates are
cross-platform lock/source packages and do not reach the import-free Wasm target. This acceptance
is a project license-policy choice; no attorney review is claimed. It is local to this provider and
does not change the permissive-only policy of the Dekopon core repository. Original source authored
here remains MIT OR Apache-2.0; the distributed combined Wasm also contains LGPL-covered code.

The owner acceptance requested for v0.1.0 is complete. The repository variable
`PROVIDER_PYTHON_RELEASE_APPROVED=true` may be set after the remote exists as a per-repository
mechanical interlock; it is not a request to repeat the policy decision.

## Corresponding source and relinkability

`scripts/build-source-bundle.sh` deterministically creates
`dekopon-python-provider-0.1.0-relink-source.tar.gz` from the immutable release commit. It contains:

- exact provider application and test source, `wit/provider.wit`, `Cargo.toml`, and `Cargo.lock`;
- the pinned toolchain, Cargo target configuration, `build.rs`, all build/validation/release
  scripts, workflows, and deployment documentation;
- MIT, Apache, LGPL 2.1, LGPL 3.0, and GPL 3.0 texts plus all project notices;
- a complete `cargo vendor --locked --versioned-dirs` closure for every registry package in the
  lockfile, including exact complete Malachite 0.9.2 sources and their upstream license files;
- an offline Cargo source replacement, deterministic `SOURCE_MANIFEST.json`, a SHA-256 inventory
  of every source file, and a reproducible CycloneDX 1.5 JSON SBOM made by exactly
  `cargo-cyclonedx` 0.5.9 with `SOURCE_DATE_EPOCH=0`.

No vendor tree, source archive, SBOM, checksum, temporary build tree, or binary is committed. CI
unpacks the archive in a new temporary directory, validates the complete vendored closure, applies
a harmless change to `malachite-base 0.9.2`, refreshes Cargo's vendored file checksum, rebuilds
from an empty canonical target with offline source replacement, componentizes with
`wasm-tools 1.236.1`, and
proves a valid import-free component results. `RELINKING.md` gives recipients the same commands and
installation information without a proprietary tool or key.

## Immutable release asset set

The v0.1.0 GitHub Release deliberately contains exactly these 14 assets:

1. `python-provider.wasm`
2. `python-provider.wasm.sha256`
3. `dekopon-python-provider-0.1.0-relink-source.tar.gz`
4. `dekopon-python-provider-0.1.0-relink-source.tar.gz.sha256`
5. `dekopon-python-provider-0.1.0.cdx.json`
6. `THIRD_PARTY_NOTICES.md`
7. `RELEASE_COMPLIANCE.md`
8. `RELINKING.md`
9. `LICENSE-MIT`
10. `LICENSE-APACHE`
11. `LICENSE-LGPL-2.1`
12. `LICENSE-LGPL-3.0`
13. `LICENSE-GPL-3.0`
14. `SHA256SUMS`

`SHA256SUMS` covers every other asset; the component and source archive also have dedicated checksum
files. The release workflow re-downloads every draft asset by asset ID and verifies names, bytes,
checksums, source contents, licenses, SBOM, and the import-free component before publication.

## OCI layout and links

The host-consumable artifact remains exactly one application layer:

- `ghcr.io/dekopon-agents/provider-python:0.1.0`
- artifact type `application/vnd.dekopon.provider.v1+wasm`
- exactly one `application/wasm` layer, `python-provider.wasm`.

It has immutable annotations linking the release source archive and
`ghcr.io/dekopon-agents/provider-python-source:0.1.0`, including the source manifest digest. Both
OCI manifests carry the exact accepted `org.opencontainers.image.licenses=LGPL-3.0-only`
annotation; the notices and SBOM provide the complete mixed-license inventory. The separate public
source artifact carries every release asset except the Wasm itself, including the Wasm checksum
that binds the two artifacts. It uses artifact type
`application/vnd.dekopon.provider.source.v1` and exact per-file media types. Neither repository
publishes `latest` or another mutable alias.

## Transaction and rollback gates

The tag workflow will proceed only when all of these hold:

- repository identity is exact, the variable interlock is true, `v0.1.0` is an annotated tag at the
  workflow SHA, package/tag versions match, the commit is contained in `main`, and no Wasm is
  tracked;
- the full source/security/resource/host suite, cargo-deny policy, notice/license hashes,
  deterministic component rebuild, deterministic source-bundle rebuild, and modified-Malachite
  offline relink test pass with pinned tools;
- no GitHub Release or final provider/source OCI version already exists, and the prior visibility
  of each authorized package is recorded in the run-marked draft before package mutation;
- the draft contains the exact 14 re-downloaded assets;
- the source manifest is pushed directly to its sole final `0.1.0` tag with exact layer counts,
  media types, byte digests, source/license metadata, immutable version/revision, and the unique run
  annotation; it is made public and every source/compliance byte is verified anonymously;
- only after that source verification is the one-layer provider manifest pushed directly to its
  sole final `0.1.0` tag, linked to the known source digest, made public, and anonymously pulled;
  both linked artifact byte sets are then recombined and checked successfully;
- the run-owned GitHub Release remains a draft throughout those checks and is finalized only after
  both final OCI manifests and all anonymous source/provider bytes have passed. Subsequent
  anonymous release checks are read-only.

A failure or cancellation before release finalization invokes run-owned cleanup. Cleanup resolves
only the two deterministic final refs and their known digests, verifies this run's annotation and
exact source/license/version metadata, and refuses to delete a manifest carrying any additional
tag. It deletes the provider before the source, restores each package's recorded prior visibility
where applicable, and deletes only this run's marked draft. It never deletes another release or
unrelated package state. The workflow creates no second tag for either manifest digest. Once the
marked release is no longer a draft, cleanup preserves the immutable finalized release and both
artifacts; a failing post-finalization read-only check is reported without destructive rollback.

Do not create the remote, set the variable, tag, push, package, or release until the owner chooses
to perform the remaining mechanical publication steps. The annotated `v0.1.0` tag must be contained
in `main`.
