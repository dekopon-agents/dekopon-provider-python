#!/usr/bin/env python3
"""Verify that vendor/ is the exact complete crates.io closure pinned by Cargo.lock."""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys
import tomllib


def fail(message: str) -> None:
    raise SystemExit(f"error: {message}")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    lock_path = root / "Cargo.lock"
    vendor = root / "vendor"
    if not lock_path.is_file() or not vendor.is_dir():
        fail(f"{root} must contain Cargo.lock and vendor/")

    packages = tomllib.loads(lock_path.read_text(encoding="utf-8"))["package"]
    expected: dict[str, dict[str, str]] = {}
    for package in packages:
        source = package.get("source", "")
        if not source.startswith("registry+https://github.com/rust-lang/crates.io-index"):
            continue
        checksum = package.get("checksum")
        if not checksum:
            fail(f"registry package {package['name']} {package['version']} has no checksum")
        directory = f"{package['name']}-{package['version']}"
        if directory in expected:
            fail(f"ambiguous versioned vendor directory {directory}")
        expected[directory] = {
            "name": package["name"],
            "version": package["version"],
            "checksum": checksum,
        }

    actual = {path.name for path in vendor.iterdir() if path.is_dir()}
    missing = sorted(set(expected) - actual)
    extra = sorted(actual - set(expected))
    if missing or extra:
        fail(f"vendor closure mismatch; missing={missing}, extra={extra}")

    for directory, package in sorted(expected.items()):
        package_root = vendor / directory
        checksum_path = package_root / ".cargo-checksum.json"
        if not checksum_path.is_file():
            fail(f"{directory} has no .cargo-checksum.json")
        checksum_data = json.loads(checksum_path.read_text(encoding="utf-8"))
        if checksum_data.get("package") != package["checksum"]:
            fail(f"{directory} archive checksum does not match Cargo.lock")
        recorded = checksum_data.get("files")
        if not isinstance(recorded, dict):
            fail(f"{directory} has an invalid file checksum map")

        files = {
            path.relative_to(package_root).as_posix()
            for path in package_root.rglob("*")
            if path.is_file() and path.name != ".cargo-checksum.json"
        }
        if files != set(recorded):
            omitted = sorted(files - set(recorded))
            absent = sorted(set(recorded) - files)
            fail(f"{directory} file inventory mismatch; omitted={omitted}, absent={absent}")
        for relative, expected_hash in sorted(recorded.items()):
            actual_hash = sha256(package_root / relative)
            if actual_hash != expected_hash:
                fail(f"{directory}/{relative} checksum mismatch")

    required = {
        "malachite-base-0.9.2": "a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e",
        "malachite-bigint-0.9.2": "cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef",
        "malachite-nz-0.9.2": "a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d",
        "malachite-q-0.9.2": "5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94",
        "r-efi-5.3.0": "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f",
        "r-efi-6.0.0": "f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf",
    }
    for directory, checksum in required.items():
        if expected.get(directory, {}).get("checksum") != checksum:
            fail(f"required LGPL package drift: {directory}")

    print(f"verified {len(expected)} exact vendored registry packages")


if __name__ == "__main__":
    main()
