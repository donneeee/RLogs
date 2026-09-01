#!/usr/bin/env python3
"""Extract named profile icons from reviewed BPSR Unity package files."""

from __future__ import annotations

import argparse
import re
import struct
from pathlib import Path

import UnityPy  # type: ignore


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--container", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--name", action="append", required=True)
    args = parser.parse_args()

    wanted = {name.casefold(): name for name in args.name}
    found: set[str] = set()
    args.output.mkdir(parents=True, exist_ok=True)
    address_catalog = (args.container / "m0.pkg").read_bytes()
    entries = read_meta_entries((args.container / "meta.pkg").read_bytes())
    bundle_hashes: dict[int, list[str]] = {}
    for requested_name in args.name:
        pattern = re.compile(
            rb"address:" + re.escape(requested_name.encode()) + rb" ->>>> hash:\d+ ->>>> bundleHash:(\d+)"
        )
        matches = {int(match) for match in pattern.findall(address_catalog)}
        if len(matches) != 1:
            raise SystemExit(f"expected one address row for {requested_name}, observed {len(matches)}")
        bundle_hashes.setdefault(matches.pop(), []).append(requested_name)

    for bundle_hash, bundle_names in bundle_hashes.items():
        matching_entries = [entry for entry in entries if entry[0] == bundle_hash]
        if len(matching_entries) != 1:
            raise SystemExit(f"expected one meta entry for bundle {bundle_hash}, observed {len(matching_entries)}")
        _, package_index, offset, length = matching_entries[0]
        package = args.container / f"m{package_index}.pkg"
        with package.open("rb") as handle:
            handle.seek(offset)
            bundle = handle.read(length)
        if len(bundle) != length or not bundle.startswith(b"UnityFS"):
            raise SystemExit(f"invalid Unity bundle {bundle_hash} in {package.name}")
        environment = UnityPy.load(bundle)
        for obj in environment.objects:
            if obj.type.name not in {"Sprite", "Texture2D"}:
                continue
            try:
                value = obj.read()
                name = str(getattr(value, "m_Name", ""))
                key = name.casefold()
                if key not in wanted or key in found:
                    continue
                image = value.image
                target = args.output / f"{wanted[key]}.png"
                image.save(target)
                found.add(key)
                print(f"{package.name}/{bundle_hash}: {obj.type.name} {name} -> {target}")
            except Exception:
                continue

    missing = sorted(wanted[key] for key in wanted.keys() - found)
    if missing:
        raise SystemExit(f"missing requested icons: {', '.join(missing)}")


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
    entries: list[tuple[int, int, int, int]] = []
    for _section in range(2):
        (count,) = take("<i")
        for _index in range(count):
            key, entry_type, package_index, entry_offset, length = take("<IBHii")
            if entry_type == 0:
                entries.append((key, package_index, entry_offset, length))
    return entries


if __name__ == "__main__":
    main()
