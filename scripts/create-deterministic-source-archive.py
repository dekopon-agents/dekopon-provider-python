#!/usr/bin/env python3
"""Create a byte-reproducible gzip-compressed source tar archive."""

from __future__ import annotations

import gzip
import io
import os
import pathlib
import stat
import sys
import tarfile


def fail(message: str) -> None:
    raise SystemExit(f"error: {message}")


def normalized_info(name: str, mode: int, kind: bytes) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.type = kind
    info.mode = mode
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    info.mtime = 0
    return info


def main() -> None:
    if len(sys.argv) != 4:
        fail("usage: create-deterministic-source-archive.py SOURCE_ROOT TOP_LEVEL OUTPUT.tar.gz")
    root = pathlib.Path(sys.argv[1]).resolve()
    top_level = sys.argv[2]
    output = pathlib.Path(sys.argv[3]).resolve()
    if not root.is_dir():
        fail(f"not a source directory: {root}")
    if not top_level or "/" in top_level or top_level in {".", ".."}:
        fail("TOP_LEVEL must be one safe path segment")
    if output == root or root in output.parents:
        fail("archive output must be outside SOURCE_ROOT")

    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".tmp")
    temporary.unlink(missing_ok=True)
    try:
        with temporary.open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.GNU_FORMAT) as archive:
                    archive.addfile(normalized_info(top_level, 0o755, tarfile.DIRTYPE))
                    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
                        relative = path.relative_to(root).as_posix()
                        name = f"{top_level}/{relative}"
                        status = path.lstat()
                        if stat.S_ISLNK(status.st_mode):
                            fail(f"source bundle must not contain symlinks: {relative}")
                        if stat.S_ISDIR(status.st_mode):
                            archive.addfile(normalized_info(name, 0o755, tarfile.DIRTYPE))
                            continue
                        if not stat.S_ISREG(status.st_mode):
                            fail(f"unsupported source file type: {relative}")
                        mode = 0o755 if status.st_mode & 0o111 else 0o644
                        info = normalized_info(name, mode, tarfile.REGTYPE)
                        info.size = status.st_size
                        with path.open("rb") as source:
                            archive.addfile(info, source)
        os.replace(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"created deterministic source archive {output} ({output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
