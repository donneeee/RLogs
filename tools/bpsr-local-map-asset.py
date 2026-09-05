#!/usr/bin/env python3
"""Compile the installed BPSR dungeon-map texture into rLogs' local-only asset namespace."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
from pathlib import Path

import UnityPy  # type: ignore

ADDRESS = "ui/textures/map/dungeon_map_bg"
OBJECT_NAME = "dungeon_map_bg"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--container", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--build", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9._/-]{1,128}", args.build) or ".." in args.build:
        raise SystemExit("build must be a safe exact client-build identity")

    address_catalog = (args.container / "m0.pkg").read_bytes()
    pattern = re.compile(
        rb"address:" + re.escape(ADDRESS.encode()) + rb" ->>>> hash:\d+ ->>>> bundleHash:(\d+)"
    )
    hashes = {int(match) for match in pattern.findall(address_catalog)}
    if len(hashes) != 1:
        raise SystemExit(f"expected one exact address row for {ADDRESS}, observed {len(hashes)}")
    bundle_hash = hashes.pop()
    entries = [entry for entry in read_meta_entries((args.container / "meta.pkg").read_bytes()) if entry[0] == bundle_hash]
    if len(entries) != 1:
        raise SystemExit(f"expected one meta entry for bundle {bundle_hash}, observed {len(entries)}")
    _, package_index, offset, length = entries[0]
    package = args.container / f"m{package_index}.pkg"
    with package.open("rb") as handle:
        handle.seek(offset)
        bundle = handle.read(length)
    if len(bundle) != length or not bundle.startswith(b"UnityFS"):
        raise SystemExit(f"invalid Unity bundle {bundle_hash} in {package.name}")

    matches = []
    for obj in UnityPy.load(bundle).objects:
        if obj.type.name != "Texture2D":
            continue
        value = obj.read()
        if str(getattr(value, "m_Name", "")) == OBJECT_NAME:
            matches.append(value)
    if len(matches) != 1:
        raise SystemExit(f"expected one Texture2D named {OBJECT_NAME}, observed {len(matches)}")

    output = args.runtime_root / args.build / "dungeon_map_bg.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    matches[0].image.save(output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    manifest = {
        "schema_version": 1,
        "game_build": args.build,
        "source_address": ADDRESS,
        "source_object": OBJECT_NAME,
        "source_bundle_hash": bundle_hash,
        "source_package": package.name,
        "asset": "dungeon_map_bg.png",
        "sha256": digest,
        "width": matches[0].image.width,
        "height": matches[0].image.height,
        "upload_allowed": False,
    }
    manifest_path = output.parent / "catalog.v1.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"{ADDRESS} ({package.name}/{bundle_hash}) -> {output}")
    print(f"local catalog -> {manifest_path}")


def read_meta_entries(data: bytes) -> list[tuple[int, int, int, int]]:
    offset = 0

    def take(fmt: str) -> tuple[int, ...]:
        nonlocal offset
        size = struct.calcsize(fmt)
        if offset + size > len(data):
            raise SystemExit("meta.pkg ended early")
        values = struct.unpack_from(fmt, data, offset)
        offset += size
        return values

    take("<iii")
    offset += 8
    take("<I")
    (header_count,) = take("<H")
    offset += 16 * header_count
    entries = []
    for _section in range(2):
        (count,) = take("<i")
        for _index in range(count):
            key, entry_type, package_index, entry_offset, length = take("<IBHii")
            if entry_type == 0:
                entries.append((key, package_index, entry_offset, length))
    return entries


if __name__ == "__main__":
    main()
