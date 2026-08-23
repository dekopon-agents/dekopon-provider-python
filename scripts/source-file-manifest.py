#!/usr/bin/env python3
"""Create or verify the deterministic per-file SHA-256 manifest in a source bundle."""

from __future__ import annotations

import hashlib
import pathlib
import sys

MANIFEST = "SOURCE_FILE_SHA256SUMS"


def fail(message: str) -> None:
    raise SystemExit(f"error: {message}")


def digest(path: pathlib.Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def inventory(root: pathlib.Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            fail(f"source bundle must not contain symlinks: {path.relative_to(root)}")
        if path.is_file() and path.name != MANIFEST:
            relative = path.relative_to(root).as_posix()
            if "\n" in relative or "\r" in relative or "\\" in relative:
                fail(f"unsupported source path: {relative!r}")
            result[relative] = digest(path)
    return result


def main() -> None:
    if len(sys.argv) != 3 or sys.argv[1] not in {"create", "verify"}:
        fail("usage: source-file-manifest.py {create|verify} SOURCE_ROOT")
    operation = sys.argv[1]
    root = pathlib.Path(sys.argv[2]).resolve()
    if not root.is_dir():
        fail(f"not a source directory: {root}")
    manifest_path = root / MANIFEST

    if operation == "create":
        files = inventory(root)
        manifest_path.write_text(
            "".join(f"{checksum}  {relative}\n" for relative, checksum in sorted(files.items())),
            encoding="utf-8",
        )
        print(f"recorded {len(files)} source files in {manifest_path}")
        return

    if not manifest_path.is_file():
        fail(f"missing {manifest_path}")
    expected: dict[str, str] = {}
    for line_number, line in enumerate(manifest_path.read_text(encoding="utf-8").splitlines(), 1):
        if len(line) < 67 or line[64:66] != "  ":
            fail(f"invalid source manifest line {line_number}")
        checksum, relative = line[:64], line[66:]
        if any(character not in "0123456789abcdef" for character in checksum):
            fail(f"invalid SHA-256 on source manifest line {line_number}")
        path = pathlib.PurePosixPath(relative)
        if path.is_absolute() or ".." in path.parts or relative in expected or relative == MANIFEST:
            fail(f"unsafe or duplicate source manifest path on line {line_number}")
        expected[relative] = checksum

    actual = inventory(root)
    if expected.keys() != actual.keys():
        missing = sorted(expected.keys() - actual.keys())
        unrecorded = sorted(actual.keys() - expected.keys())
        fail(f"source file inventory mismatch; missing={missing}, unrecorded={unrecorded}")
    for relative, expected_hash in expected.items():
        if actual[relative] != expected_hash:
            fail(f"source file checksum mismatch: {relative}")
    print(f"verified {len(actual)} source files from {manifest_path}")


if __name__ == "__main__":
    main()
