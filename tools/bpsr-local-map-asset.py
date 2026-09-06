#!/usr/bin/env python3
"""Compile a reviewed BPSR map texture into rLogs' local-only asset namespace."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
from pathlib import Path

import UnityPy  # type: ignore

DEFAULT_ADDRESS = "ui/textures/map/dungeon_map_bg"
DEFAULT_OBJECT_NAME = "dungeon_map_bg"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--container", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--build", required=True)
    parser.add_argument("--address", default=DEFAULT_ADDRESS)
    parser.add_argument("--object-name", default=DEFAULT_OBJECT_NAME)
    parser.add_argument("--asset", default="dungeon_map_bg.png")
    parser.add_argument("--region-address")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9._/-]{1,128}", args.build) or ".." in args.build:
        raise SystemExit("build must be a safe exact client-build identity")
    if not re.fullmatch(r"[A-Za-z0-9._/-]{1,240}", args.address) or ".." in args.address:
        raise SystemExit("address must be a safe exact game-asset address")
    if not re.fullmatch(r"[A-Za-z0-9._-]{1,192}", args.object_name):
        raise SystemExit("object-name must be a safe exact Texture2D name")
    if not re.fullmatch(r"[A-Za-z0-9._-]{1,128}\.png", args.asset):
        raise SystemExit("asset must be a safe PNG file name")
    if args.region_address and (
        not re.fullmatch(r"[A-Za-z0-9._/-]{1,240}", args.region_address)
        or ".." in args.region_address
    ):
        raise SystemExit("region-address must be a safe exact game-asset address")

    address_catalog = (args.container / "m0.pkg").read_bytes()
    pattern = re.compile(
        rb"address:" + re.escape(args.address.encode()) + rb" ->>>> hash:\d+ ->>>> bundleHash:(\d+)"
    )
    hashes = {int(match) for match in pattern.findall(address_catalog)}
    if len(hashes) != 1:
        raise SystemExit(f"expected one exact address row for {args.address}, observed {len(hashes)}")
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
        if str(getattr(value, "m_Name", "")) == args.object_name:
            matches.append(value)
    if len(matches) != 1:
        raise SystemExit(
            f"expected one Texture2D named {args.object_name}, observed {len(matches)}"
        )

    output = args.runtime_root / args.build / args.asset
    output.parent.mkdir(parents=True, exist_ok=True)
    matches[0].image.save(output)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    manifest = {
        "schema_version": 1,
        "game_build": args.build,
        "source_address": args.address,
        "source_object": args.object_name,
        "source_bundle_hash": bundle_hash,
        "source_package": package.name,
        "asset": args.asset,
        "sha256": digest,
        "width": matches[0].image.width,
        "height": matches[0].image.height,
        "upload_allowed": False,
    }
    if args.region_address:
        region_bundle_hash, region_package, region_bundle = read_address_bundle(
            args.container, args.region_address
        )
        region_objects = [
            obj
            for obj in UnityPy.load(region_bundle).objects
            if obj.type.name == "MonoBehaviour"
        ]
        if len(region_objects) != 1:
            raise SystemExit(
                f"expected one region-data MonoBehaviour, observed {len(region_objects)}"
            )
        raw = region_objects[0].get_raw_data()
        if len(raw) < 36:
            raise SystemExit("region-data payload ended before its map transform")
        values = struct.unpack("<9f", raw[-36:])
        if values[7] <= 0 or values[8] <= 0:
            raise SystemExit("region-data map span must be positive")
        manifest["region_transform"] = {
            "source_address": args.region_address,
            "source_bundle_hash": region_bundle_hash,
            "source_package": region_package.name,
            "world_origin": {"x": values[4], "y": values[5], "z": values[6]},
            "world_span": {"x": values[7], "z": values[8]},
            "raw_prefix_values": list(values[:4]),
        }
    manifest_path = (
        output.parent / "catalog.v1.json"
        if args.asset == "dungeon_map_bg.png"
        else output.with_suffix(".catalog.v1.json")
    )
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"{args.address} ({package.name}/{bundle_hash}) -> {output}")
    print(f"local catalog -> {manifest_path}")


def read_address_bundle(container: Path, address: str) -> tuple[int, Path, bytes]:
    address_catalog = (container / "m0.pkg").read_bytes()
    pattern = re.compile(
        rb"address:" + re.escape(address.encode()) + rb" ->>>> hash:\d+ ->>>> bundleHash:(\d+)"
    )
    hashes = {int(match) for match in pattern.findall(address_catalog)}
    if len(hashes) != 1:
        raise SystemExit(f"expected one exact address row for {address}, observed {len(hashes)}")
    bundle_hash = hashes.pop()
    entries = [
        entry
        for entry in read_meta_entries((container / "meta.pkg").read_bytes())
        if entry[0] == bundle_hash
    ]
    if len(entries) != 1:
        raise SystemExit(
            f"expected one meta entry for bundle {bundle_hash}, observed {len(entries)}"
        )
    _, package_index, offset, length = entries[0]
    package = container / f"m{package_index}.pkg"
    with package.open("rb") as handle:
        handle.seek(offset)
        bundle = handle.read(length)
    if len(bundle) != length or not bundle.startswith(b"UnityFS"):
        raise SystemExit(f"invalid Unity bundle {bundle_hash} in {package.name}")
    return bundle_hash, package, bundle


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
