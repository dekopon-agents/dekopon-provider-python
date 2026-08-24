#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
out=${1:-"$root/dist"}
required_rust=1.97.0
required_rustc='rustc 1.97.0 (2d8144b78 2026-07-07)'
required_cyclonedx=0.5.9

[[ "$(rustup run "$required_rust" rustc --version)" == "$required_rustc" ]] || {
  echo "error: expected $required_rustc" >&2
  exit 1
}
[[ "$(cargo cyclonedx --version)" == "cargo-cyclonedx-cyclonedx $required_cyclonedx" ]] || {
  echo "error: expected cargo-cyclonedx $required_cyclonedx" >&2
  exit 1
}
git -C "$root" diff --quiet
git -C "$root" diff --cached --quiet
test -z "$(git -C "$root" ls-files '*.wasm')" || {
  echo 'error: generated Wasm must never be tracked' >&2
  exit 1
}

read -r package version < <(python3 - "$root/Cargo.toml" <<'PY'
import pathlib, sys, tomllib
package = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))["package"]
print(package["name"], package["version"])
PY
)
[[ "$package" == dekopon-python-provider && "$version" == 0.1.0 ]] || {
  echo "error: immutable source bundle expects dekopon-python-provider 0.1.0" >&2
  exit 1
}
revision=$(git -C "$root" rev-parse HEAD)
[[ "$revision" =~ ^[0-9a-f]{40}$ ]]

top="dekopon-python-provider-$version"
archive="$top-relink-source.tar.gz"
sbom="$top.cdx.json"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/dekopon-python-source.XXXXXX")
cleanup() {
  rm -rf "$temporary"
}
trap cleanup EXIT
stage="$temporary/$top"
mkdir -p "$stage" "$out"
git -C "$root" archive --format=tar HEAD | tar -xf - -C "$stage"

# Cargo's versioned vendor layout is deterministic and includes every registry package selected by
# the complete lockfile (normal, build, development, and target-specific closure). The generated
# source replacement makes the unpacked archive self-contained for Cargo dependency resolution.
cargo +"$required_rust" vendor --locked --versioned-dirs \
  --manifest-path "$stage/Cargo.toml" "$stage/vendor" >"$temporary/vendor-config.toml"
cat >>"$stage/.cargo/config.toml" <<'EOF'

# Generated only in the corresponding-source archive. Keep dependency resolution offline and
# rooted in the exact crates.io sources carried in vendor/.
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"

[net]
offline = true
EOF

python3 "$stage/scripts/verify-vendored-source.py" "$stage"
(
  cd "$stage"
  cargo +"$required_rust" metadata --locked --offline --format-version 1 >/dev/null
  SOURCE_DATE_EPOCH=0 cargo +"$required_rust" cyclonedx \
    --manifest-path Cargo.toml \
    --format json \
    --spec-version 1.5 \
    --target wasm32-unknown-unknown \
    --override-filename "${top}.cdx"
)
[[ -f "$stage/$sbom" ]]
# cargo-cyclonedx intentionally identifies the local workspace with an absolute path. Normalize
# only that first-party bom-ref prefix so independently staged source trees produce identical SBOM
# bytes; registry package identities and checksums remain untouched.
python3 - "$stage/$sbom" "$stage" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
stage = pathlib.Path(sys.argv[2]).resolve().as_posix()
text = path.read_text(encoding="utf-8")
old = f"path+file://{stage}"
if old not in text:
    raise SystemExit("error: cargo-cyclonedx SBOM lacks the expected workspace bom-ref")
text = text.replace(old, "path+file:///dekopon/source")
if stage in text:
    raise SystemExit("error: generated SBOM retains its temporary staging path")
json.loads(text)
path.write_text(text, encoding="utf-8")
PY

python3 - "$stage/Cargo.lock" "$stage/$sbom" "$stage/SOURCE_MANIFEST.json" \
  "$revision" "$version" "$archive" "$sbom" <<'PY'
import json, pathlib, sys, tomllib
lock_path, sbom_path, output, revision, version, archive, sbom_name = sys.argv[1:]
packages = tomllib.loads(pathlib.Path(lock_path).read_text(encoding="utf-8"))["package"]
registry = [p for p in packages if p.get("source", "").startswith("registry+")]
sbom = json.loads(pathlib.Path(sbom_path).read_text(encoding="utf-8"))
components = {(item.get("name"), item.get("version")) for item in sbom.get("components", [])}
for name in ("malachite-base", "malachite-bigint", "malachite-nz", "malachite-q"):
    if (name, "0.9.2") not in components:
        raise SystemExit(f"error: SBOM omits {name} 0.9.2")
manifest = {
    "formatVersion": 1,
    "package": "dekopon-python-provider",
    "version": version,
    "gitRevision": revision,
    "archive": archive,
    "sbom": sbom_name,
    "rustToolchain": "1.97.0",
    "wasmTarget": "wasm32-unknown-unknown",
    "wasmTools": "1.236.1",
    "cargoCycloneDx": "0.5.9",
    "lockedPackageCount": len(packages),
    "vendoredRegistryPackageCount": len(registry),
    "dependencySourceMode": "complete versioned Cargo vendor closure; offline source replacement",
    "relinkInstructions": "RELINKING.md",
    "lgplPackages": [
        {"name": "malachite-base", "version": "0.9.2", "sha256": "a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e"},
        {"name": "malachite-bigint", "version": "0.9.2", "sha256": "cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef"},
        {"name": "malachite-nz", "version": "0.9.2", "sha256": "a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d"},
        {"name": "malachite-q", "version": "0.9.2", "sha256": "5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94"},
        {"name": "r-efi", "version": "5.3.0", "sha256": "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f"},
        {"name": "r-efi", "version": "6.0.0", "sha256": "f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf"},
    ],
}
pathlib.Path(output).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

for license in LICENSE-LGPL-2.1 LICENSE-LGPL-3.0 LICENSE-GPL-3.0; do
  [[ -f "$stage/$license" ]] || { echo "error: source archive lacks $license" >&2; exit 1; }
done
echo '20e50fe7aae3e56378ebf0417d9de904f55a0e61e4df315333e632a4d3555d95  LICENSE-LGPL-2.1' |
  (cd "$stage" && sha256sum_check -)
echo 'e3a994d82e644b03a792a930f574002658412f62407f5fee083f2555c5f23118  LICENSE-LGPL-3.0' |
  (cd "$stage" && sha256sum_check -)
echo '3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986  LICENSE-GPL-3.0' |
  (cd "$stage" && sha256sum_check -)

python3 "$stage/scripts/source-file-manifest.py" create "$stage"
python3 "$stage/scripts/source-file-manifest.py" verify "$stage"
rm -f "$out/$archive" "$out/$archive.sha256" "$out/$sbom"
python3 "$stage/scripts/create-deterministic-source-archive.py" \
  "$stage" "$top" "$out/$archive"
cp "$stage/$sbom" "$out/$sbom"
(
  cd "$out"
  sha256sum_run "$archive" >"$archive.sha256"
  sha256sum_check "$archive.sha256"
)
printf 'source bundle: %s\nSBOM: %s\nrevision: %s\n' \
  "$out/$archive" "$out/$sbom" "$revision"
