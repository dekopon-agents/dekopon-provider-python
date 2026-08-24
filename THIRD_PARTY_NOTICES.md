# Third-party notices

## Important notice for every binary recipient

`dekopon-python-provider` source written by this project is licensed under **MIT OR Apache-2.0**.
The distributed `python-provider.wasm` is a combined WebAssembly component that statically embeds
third-party code, including the four **LGPL-3.0-only** Malachite 0.9.2 packages below. The
permissive license on original project source does not replace those embedded-code terms.

Every v0.1.0 binary copy is paired, at no charge and without authentication, with the exact
corresponding-source/relink archive, its SHA-256, a CycloneDX SBOM, this notice, relinking
instructions, and verbatim GNU license texts. Obtain them from the `v0.1.0` GitHub Release or
`ghcr.io/dekopon-agents/provider-python-source:0.1.0`; see `RELINKING.md`. The archive permits a
recipient to modify/replace Malachite and rebuild/componentize a modified provider from complete
offline dependency source. No `latest` tag is published.

This inventory records the repository owner's accepted v0.1.0 license-policy design. It is not
legal advice, does not claim attorney review, and does not alter the permissive-only policy of the
separate Dekopon core repository.

## Accepted exact LGPL packages

The exact RustPython 0.5.0 graph embedded in the Wasm necessarily contains:

| Package | License | crates.io archive SHA-256 |
|---|---|---|
| `malachite-base 0.9.2` | LGPL-3.0-only | `a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e` |
| `malachite-bigint 0.9.2` | LGPL-3.0-only | `cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef` |
| `malachite-nz 0.9.2` | LGPL-3.0-only | `a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d` |
| `malachite-q 0.9.2` | LGPL-3.0-only | `5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94` |

Upstream is <https://github.com/mhogrefe/malachite>. `Cargo.lock` identifies the exact crates.io
archives; the corresponding-source archive contains their complete unpacked source and upstream
license files. `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` are verbatim texts from the Free Software
Foundation. Malachite modifications and redistribution remain subject to those terms.

The complete cross-platform lock/source closure also contains `r-efi 5.3.0`
(`69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f`) and `r-efi 6.0.0`
(`f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf`) under
**LGPL-2.1-or-later**. They do not reach the `wasm32-unknown-unknown` component graph or introduce a
component import. They remain narrowly accepted and inventoried rather than hidden by a blanket
LGPL allowance. `LICENSE-LGPL-2.1` provides the verbatim version 2.1 text; recipients may use the
package's “or later” option.

## RustPython and Python library material

- `rustpython-vm 0.5.0` and `rustpython-stdlib 0.5.0`, together with the synchronized
  `rustpython-* 0.5.0` implementation crates, are from
  <https://github.com/RustPython/RustPython> and use RustPython's MIT terms. The complete
  `rustpython-derive-impl 0.5.0` source in `patches/rustpython-derive-impl` carries a local
  reproducibility fix that sorts frozen-module traversal and map/set-backed macro token emission
  without changing Python or Rust code or data; `README.dekopon.md` records the original crate
  checksum and exact modification.
- `rustpython-pylib 0.5.0` embeds Python library material under the Python Software Foundation
  License / `Python-2.0.1`. Its authoritative text is `Lib/PSF-LICENSE` in that exact crate source.
- `rustpython-doc 0.5.0` carries `Python-2.0.1`; `deny.toml` pins the detected license-file hash.
- `pymath` is `PSF-2.0`.

RustPython includes code and data derived from CPython, Unicode, and other projects. Their notices
and license files in the exact vendored crates.io source remain applicable. The lockfile pins all
synchronized RustPython codegen/common/compiler/compiler-core/derive/doc/literal/pylib/
sre-engine/stdlib/vm/wtf8 crates to 0.5.0.

## YAML

`yaml-rust2 0.12.0` is Copyright Yuheng Chen, Ethiraric, David Aguilar, and contributors, licensed
under **MIT OR Apache-2.0**. The provider uses it with default features disabled. Upstream:
<https://github.com/Ethiraric/yaml-rust2>.

## Unicode and BSD-style material

The graph contains Unicode data and implementations under `Unicode-3.0`, `Unicode-DFS-2016`,
BSD-2-Clause, BSD-2-Clause-Views, BSD-3-Clause, ISC, and PSF terms. In particular the exact
`unic-* 0.9.0` family (including `unic-char-property`) and `unicode_names2` data are retained. The
complete machine-checked license inventory is produced by:

```console
cargo deny list
cargo deny check licenses
```

The `unic-*` family is unmaintained (the exact advisories are listed in `deny.toml`) but unavoidable
in RustPython 0.5.0. Those targeted acknowledgements are reviewed separately from license policy.

## Known unmaintained transitive code

`paste 1.0.15` is transitively required by the exact RustPython/Malachite graph and is covered by
`RUSTSEC-2024-0436`. The `unic-*` advisories are individually named in `deny.toml`. There is no
blanket advisory ignore; upgrading or replacing these dependencies requires moving away from the
exact tested RustPython 0.5.0 graph.

## License texts and reproducible inventory

The release and corresponding-source archive carry:

- `LICENSE-MIT` and `LICENSE-APACHE` for project-authored source;
- `LICENSE-LGPL-2.1` (authoritative SHA-256
  `20e50fe7aae3e56378ebf0417d9de904f55a0e61e4df315333e632a4d3555d95`);
- `LICENSE-LGPL-3.0` (authoritative SHA-256
  `e3a994d82e644b03a792a930f574002658412f62407f5fee083f2555c5f23118`);
- `LICENSE-GPL-3.0` (authoritative SHA-256
  `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`);
- every package-specific license/notice file in the complete vendored dependency source.

Run `./scripts/assert-lock-and-feature-graph.sh Cargo.lock`,
`./scripts/check-third-party-notices.sh Cargo.lock THIRD_PARTY_NOTICES.md`, and
`./scripts/build-source-bundle.sh dist`. `Cargo.lock` is the authority for exact package versions
and archive checksums; `SOURCE_FILE_SHA256SUMS` is the authority for unpacked source bytes. No
generated Wasm, archive, SBOM, checksum, or vendor tree is source-controlled.
