#!/usr/bin/env python3
"""Verify the immutable provider or corresponding-source OCI manifest."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import sys

VERSION = "0.1.0"
SOURCE_ARCHIVE = "dekopon-python-provider-0.1.0-relink-source.tar.gz"
SOURCE_REPOSITORY = "ghcr.io/dekopon-agents/provider-python-source"
PROVIDER_REPOSITORY = "ghcr.io/dekopon-agents/provider-python"
RELEASE = "https://github.com/dekopon-agents/dekopon-provider-python/releases/tag/v0.1.0"
OCI_LICENSES = "LGPL-3.0-only"
SOURCE_MEDIA = {
    "python-provider.wasm.sha256": "text/plain",
    SOURCE_ARCHIVE: "application/gzip",
    f"{SOURCE_ARCHIVE}.sha256": "text/plain",
    "dekopon-python-provider-0.1.0.cdx.json": "application/vnd.cyclonedx+json",
    "THIRD_PARTY_NOTICES.md": "text/markdown",
    "RELEASE_COMPLIANCE.md": "text/markdown",
    "RELINKING.md": "text/markdown",
    "LICENSE-MIT": "text/plain",
    "LICENSE-APACHE": "text/plain",
    "LICENSE-LGPL-2.1": "text/plain",
    "LICENSE-LGPL-3.0": "text/plain",
    "LICENSE-GPL-3.0": "text/plain",
    "SHA256SUMS": "text/plain",
}


def fail(message: str) -> None:
    raise SystemExit(f"error: {message}")


def digest(path: pathlib.Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def require_annotations(actual: dict[str, str], expected: dict[str, str]) -> None:
    for key, value in expected.items():
        if actual.get(key) != value:
            fail(f"OCI annotation {key!r} mismatch: {actual.get(key)!r} != {value!r}")
    if any("latest" in value.lower() for value in actual.values()):
        fail("OCI annotations must not refer to latest")


def main() -> None:
    if len(sys.argv) not in {6, 7} or sys.argv[1] not in {"provider", "source"}:
        fail("usage: verify-oci-manifest.py {provider|source} MANIFEST DIST RUN REVISION [SOURCE_DIGEST]")
    kind, manifest_path, dist_path, run, revision = sys.argv[1:6]
    source_digest = sys.argv[6] if len(sys.argv) == 7 else ""
    if not re.fullmatch(r"[0-9]+:[0-9]+", run):
        fail("invalid run marker")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        fail("invalid Git revision")
    if kind == "provider" and not re.fullmatch(r"sha256:[0-9a-f]{64}", source_digest):
        fail("provider verification requires the exact source manifest digest")

    manifest = json.loads(pathlib.Path(manifest_path).read_text(encoding="utf-8"))
    dist = pathlib.Path(dist_path)
    if manifest.get("schemaVersion") != 2:
        fail("OCI manifest schemaVersion must be 2")
    config = manifest.get("config", {})
    if config.get("size") not in {0, 2}:
        fail("OCI manifest config must be empty")
    annotations = manifest.get("annotations", {})
    common = {
        "org.opencontainers.image.source": "https://github.com/dekopon-agents/dekopon-provider-python",
        "org.opencontainers.image.version": VERSION,
        "org.opencontainers.image.revision": revision,
        "org.opencontainers.image.licenses": OCI_LICENSES,
        "org.dekopon.release.run": run,
        "org.dekopon.release.url": RELEASE,
        "org.dekopon.corresponding-source.archive": SOURCE_ARCHIVE,
    }

    if kind == "provider":
        if manifest.get("artifactType") != "application/vnd.dekopon.provider.v1+wasm":
            fail("provider artifact type mismatch")
        require_annotations(
            annotations,
            common
            | {
                "org.dekopon.corresponding-source.oci": f"{SOURCE_REPOSITORY}:{VERSION}",
                "org.dekopon.corresponding-source.digest": source_digest,
                "org.dekopon.distribution.notice": "combined Wasm embeds LGPL-3.0-only Malachite 0.9.2; see THIRD_PARTY_NOTICES.md",
            },
        )
        layers = manifest.get("layers", [])
        if len(layers) != 1:
            fail("provider OCI manifest must have exactly one layer")
        layer = layers[0]
        component = dist / "python-provider.wasm"
        if (
            layer.get("mediaType") != "application/wasm"
            or layer.get("digest") != digest(component)
            or layer.get("size") != component.stat().st_size
            or layer.get("annotations", {}).get("org.opencontainers.image.title") != component.name
        ):
            fail("provider Wasm layer metadata/bytes mismatch")
        return

    if source_digest:
        fail("source verification does not accept a source digest argument")
    if manifest.get("artifactType") != "application/vnd.dekopon.provider.source.v1":
        fail("source artifact type mismatch")
    archive_hash = hashlib.sha256((dist / SOURCE_ARCHIVE).read_bytes()).hexdigest()
    require_annotations(
        annotations,
        common
        | {
            "org.dekopon.provider.oci": f"{PROVIDER_REPOSITORY}:{VERSION}",
            "org.dekopon.corresponding-source.sha256": archive_hash,
            "org.dekopon.distribution.notice": "complete corresponding source and relink materials for LGPL-covered provider",
        },
    )
    layers = manifest.get("layers", [])
    if len(layers) != len(SOURCE_MEDIA):
        fail(f"source OCI manifest must have exactly {len(SOURCE_MEDIA)} layers")
    actual: dict[str, dict] = {}
    for layer in layers:
        title = layer.get("annotations", {}).get("org.opencontainers.image.title")
        if not isinstance(title, str) or title in actual:
            fail("source OCI layer title is absent or duplicated")
        actual[title] = layer
    if actual.keys() != SOURCE_MEDIA.keys():
        fail(f"source OCI layer names mismatch: {sorted(actual)}")
    for name, media_type in SOURCE_MEDIA.items():
        path = dist / name
        layer = actual[name]
        if (
            layer.get("mediaType") != media_type
            or layer.get("digest") != digest(path)
            or layer.get("size") != path.stat().st_size
        ):
            fail(f"source OCI layer metadata/bytes mismatch: {name}")


if __name__ == "__main__":
    main()
