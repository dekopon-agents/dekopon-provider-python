#!/usr/bin/env python3
"""Refresh one Cargo vendor file hash after a recipient modifies vendored source."""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys


def fail(message: str) -> None:
    raise SystemExit(f"error: {message}")


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: refresh-vendored-checksum.py VENDORED_PACKAGE RELATIVE_FILE")

    package = pathlib.Path(sys.argv[1]).resolve()
    relative = pathlib.PurePosixPath(sys.argv[2])
    if relative.is_absolute() or ".." in relative.parts or relative.as_posix() == ".cargo-checksum.json":
        fail("RELATIVE_FILE must be a safe package-relative source path")
    checksum_path = package / ".cargo-checksum.json"
    source_path = package.joinpath(*relative.parts)
    if not checksum_path.is_file() or not source_path.is_file():
        fail("vendored package checksum or modified file is missing")
    if package.name == "vendor" or package.parent.name != "vendor":
        fail("VENDORED_PACKAGE must be one package directly below vendor/")

    data = json.loads(checksum_path.read_text(encoding="utf-8"))
    files = data.get("files")
    key = relative.as_posix()
    if not isinstance(files, dict) or key not in files:
        fail(f"{key} is not in the package's Cargo checksum inventory")
    files[key] = hashlib.sha256(source_path.read_bytes()).hexdigest()
    checksum_path.write_text(
        json.dumps(data, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    print(f"refreshed {checksum_path.relative_to(package.parent.parent)} for {key}")


if __name__ == "__main__":
    main()
