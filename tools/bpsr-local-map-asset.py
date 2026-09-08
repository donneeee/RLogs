#!/usr/bin/env python3
"""Compile a reviewed BPSR map texture into rLogs' local-only asset namespace."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
from pathlib import Path
from typing import Optional

import UnityPy  # type: ignore
from PIL import __version__ as pillow_version  # type: ignore

DEFAULT_ADDRESS = "ui/textures/map/dungeon_map_bg"
DEFAULT_OBJECT_NAME = "dungeon_map_bg"
COMPILER_VERSION = "2"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", action="store_true")
    parser.add_argument("--self-check", action="store_true")
    parser.add_argument("--reviewed-manifest", type=Path)
    parser.add_argument("--inventory-output", type=Path)
    parser.add_argument("--inventory-input", type=Path)
    parser.add_argument("--candidate-manifest-output", type=Path)
    parser.add_argument("--scene-table", type=Path)
    parser.add_argument("--scene-resource-table", type=Path)
    parser.add_argument("--container", type=Path)
    parser.add_argument("--runtime-root", type=Path)
    parser.add_argument("--build")
    parser.add_argument("--address", default=DEFAULT_ADDRESS)
    parser.add_argument("--object-name", default=DEFAULT_OBJECT_NAME)
    parser.add_argument("--asset", default="dungeon_map_bg.png")
    parser.add_argument("--region-address")
    args = parser.parse_args()
    if args.version:
        print(f"rLogs BPSR map compiler {COMPILER_VERSION}")
        return
    if args.self_check:
        run_self_check()
        return
    if args.container is None or args.runtime_root is None or args.build is None:
        if args.inventory_output is None:
            parser.error("--container, --runtime-root, and --build are required for extraction")
    if args.inventory_output is not None:
        if args.container is None or args.build is None:
            parser.error("--container and --build are required for map inventory")
        write_scene_map_inventory(
            args.container,
            args.build,
            args.inventory_output,
            args.scene_table,
            args.scene_resource_table,
        )
        return
    if args.candidate_manifest_output is not None:
        if args.container is None or args.runtime_root is None or args.build is None:
            parser.error(
                "--container, --runtime-root, and --build are required for candidate extraction"
            )
        if args.inventory_input is None:
            parser.error("--inventory-input is required for candidate extraction")
        compile_inventory_candidates(
            args.container,
            args.runtime_root,
            args.build,
            args.inventory_input,
            args.candidate_manifest_output,
        )
        return
    if args.reviewed_manifest is not None:
        compile_reviewed_manifest(
            args.container, args.runtime_root, args.build, args.reviewed_manifest
        )
        return
    compile_asset(args)


def compile_asset(
    args: argparse.Namespace,
    expected: Optional[dict] = None,
    address_catalog: Optional[bytes] = None,
    meta_entries: Optional[list[tuple[int, int, int, int]]] = None,
) -> dict:
    if not is_safe_relative_identity(args.build, 128):
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

    if address_catalog is None:
        address_catalog = (args.container / "m0.pkg").read_bytes()
    pattern = re.compile(
        rb"address:" + re.escape(args.address.encode()) + rb" ->>>> hash:\d+ ->>>> bundleHash:(\d+)"
    )
    hashes = {int(match) for match in pattern.findall(address_catalog)}
    if len(hashes) != 1:
        raise SystemExit(f"expected one exact address row for {args.address}, observed {len(hashes)}")
    bundle_hash = hashes.pop()
    if expected is not None and bundle_hash != expected["source_bundle_hash"]:
        raise SystemExit(
            f"reviewed source bundle changed for {args.asset}: "
            f"expected {expected['source_bundle_hash']}, observed {bundle_hash}"
        )
    if meta_entries is None:
        meta_entries = read_meta_entries((args.container / "meta.pkg").read_bytes())
    entries = [entry for entry in meta_entries if entry[0] == bundle_hash]
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
    image = matches[0].image
    if expected is not None and (image.width, image.height) != (
        expected["width"],
        expected["height"],
    ):
        raise SystemExit(
            f"reviewed texture dimensions changed for {args.asset}: "
            f"expected {expected['width']}x{expected['height']}, "
            f"observed {image.width}x{image.height}"
        )

    runtime_root = args.runtime_root.resolve()
    output = (runtime_root / args.build / args.asset).resolve()
    try:
        output.relative_to(runtime_root)
    except ValueError:
        raise SystemExit("output must remain inside the local runtime root")
    manifest = {
        "schema_version": 1,
        "game_build": args.build,
        "source_address": args.address,
        "source_object": args.object_name,
        "source_bundle_hash": bundle_hash,
        "source_package": package.name,
        "asset": args.asset,
        "width": image.width,
        "height": image.height,
        "upload_allowed": False,
    }
    if args.region_address:
        region_bundle_hash, region_package, region_bundle = read_address_bundle(
            args.container, args.region_address
        )
        if expected is not None and region_bundle_hash != expected["region_bundle_hash"]:
            raise SystemExit(
                f"reviewed region bundle changed for {args.asset}: "
                f"expected {expected['region_bundle_hash']}, observed {region_bundle_hash}"
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
        if expected is not None:
            observed_transform = (values[4], values[6], values[7], values[8])
            expected_transform = tuple(expected[key] for key in (
                "origin_x", "origin_z", "span_x", "span_z"
            ))
            if any(
                abs(observed - reviewed) > 0.0001
                for observed, reviewed in zip(observed_transform, expected_transform)
            ):
                raise SystemExit(
                    f"reviewed region transform changed for {args.asset}: "
                    f"expected {expected_transform!r}, observed {observed_transform!r}"
                )
        manifest["region_transform"] = {
            "source_address": args.region_address,
            "source_bundle_hash": region_bundle_hash,
            "source_package": region_package.name,
            "world_origin": {"x": values[4], "y": values[5], "z": values[6]},
            "world_span": {"x": values[7], "z": values[8]},
            "raw_prefix_values": list(values[:4]),
        }
    output.parent.mkdir(parents=True, exist_ok=True)
    pending = output.with_name(f".{output.name}.pending-{os.getpid()}")
    image.save(pending, format="PNG")
    digest = hashlib.sha256(pending.read_bytes()).hexdigest()
    pending.replace(output)
    manifest["sha256"] = digest
    manifest_path = (
        output.parent / "catalog.v1.json"
        if args.asset == "dungeon_map_bg.png"
        else output.with_suffix(".catalog.v1.json")
    )
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"{args.address} ({package.name}/{bundle_hash}) -> {output}")
    print(f"local catalog -> {manifest_path}")
    return manifest


def compile_reviewed_manifest(
    container: Path, runtime_root: Path, build: str, manifest_path: Path
) -> None:
    if not is_safe_relative_identity(build, 128):
        raise SystemExit("build must be a safe exact client-build identity")
    if manifest_path.stat().st_size > 128 * 1024:
        raise SystemExit("reviewed map manifest exceeds 128 KiB")
    value = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != {"schema_version", "builds"}:
        raise SystemExit("reviewed map manifest has an invalid root")
    if value["schema_version"] != 1 or not isinstance(value["builds"], dict):
        raise SystemExit("reviewed map manifest has an unsupported schema")
    entries = value["builds"].get(build)
    if not isinstance(entries, list) or not entries:
        raise SystemExit(f"no reviewed local map assets match exact build {build}")
    required = {
        "address", "object_name", "asset", "region_address", "source_bundle_hash",
        "region_bundle_hash", "width", "height", "origin_x", "origin_z", "span_x",
        "span_z", "scene_ids",
    }
    address_catalog = (container / "m0.pkg").read_bytes()
    meta_entries = read_meta_entries((container / "meta.pkg").read_bytes())
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != required:
            raise SystemExit("reviewed map manifest entry has invalid fields")
        if not isinstance(entry["scene_ids"], list) or not entry["scene_ids"]:
            raise SystemExit("reviewed map manifest entry has no scene IDs")
        compile_asset(
            argparse.Namespace(
                container=container,
                runtime_root=runtime_root,
                build=build,
                address=entry["address"],
                object_name=entry["object_name"],
                asset=entry["asset"],
                region_address=entry["region_address"],
            ),
            entry,
            address_catalog,
            meta_entries,
        )
    print(f"prepared {len(entries)} reviewed map assets for {build}")


def compile_inventory_candidates(
    container: Path,
    runtime_root: Path,
    build: str,
    inventory_path: Path,
    output_path: Path,
) -> None:
    """Materialize strict one-texture candidates for visual review.

    This deliberately writes a candidate manifest, never the production
    reviewed manifest. Multi-layer maps and map families without an exact
    SceneTable join remain excluded until their composition or alias is
    reviewed separately.
    """
    inventory_bytes = bounded_table_bytes(inventory_path, "map inventory")
    inventory = json.loads(inventory_bytes)
    if (
        not isinstance(inventory, dict)
        or inventory.get("schema_version") != 1
        or inventory.get("game_build") != build
        or not isinstance(inventory.get("families"), list)
    ):
        raise SystemExit("map inventory does not match the requested exact build")
    address_catalog = (container / "m0.pkg").read_bytes()
    meta_entries = read_meta_entries((container / "meta.pkg").read_bytes())
    reviewed = []
    for family in inventory["families"]:
        if not isinstance(family, dict) or not family.get("review_ready"):
            continue
        textures = family.get("texture_candidates")
        region = family.get("region_data")
        scene_ids = family.get("scene_ids")
        name = family.get("family")
        if (
            not isinstance(textures, list)
            or len(textures) != 1
            or not isinstance(region, dict)
            or not isinstance(scene_ids, list)
            or not scene_ids
            or not isinstance(name, str)
        ):
            raise SystemExit("review-ready inventory row has invalid fields")
        texture = textures[0]
        address = texture.get("address")
        region_address = region.get("address")
        if not isinstance(address, str) or not isinstance(region_address, str):
            raise SystemExit("review-ready inventory row has invalid addresses")
        object_name = address.rsplit("/", 1)[-1]
        asset_name = f"scene-map-{name}.png"
        manifest = compile_asset(
            argparse.Namespace(
                container=container,
                runtime_root=runtime_root,
                build=build,
                address=address,
                object_name=object_name,
                asset=asset_name,
                region_address=region_address,
            ),
            address_catalog=address_catalog,
            meta_entries=meta_entries,
        )
        transform = manifest.get("region_transform")
        if not isinstance(transform, dict):
            raise SystemExit(f"candidate {name} has no region transform")
        world_origin = transform["world_origin"]
        world_span = transform["world_span"]
        reviewed.append({
            "scene_ids": sorted(set(scene_ids)),
            "address": address,
            "object_name": object_name,
            "asset": asset_name,
            "region_address": region_address,
            "source_bundle_hash": manifest["source_bundle_hash"],
            "region_bundle_hash": transform["source_bundle_hash"],
            "width": manifest["width"],
            "height": manifest["height"],
            "origin_x": world_origin["x"],
            "origin_z": world_origin["z"],
            "span_x": world_span["x"],
            "span_z": world_span["z"],
        })
    candidate = {
        "schema_version": 1,
        "candidate_only": True,
        "game_build": build,
        "inventory_sha256": hashlib.sha256(inventory_bytes).hexdigest(),
        "entries": reviewed,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(candidate, indent=2) + "\n", encoding="utf-8")
    print(f"prepared {len(reviewed)} map candidates for review -> {output_path}")


def write_scene_map_inventory(
    container: Path,
    build: str,
    output: Path,
    scene_table_path: Optional[Path],
    scene_resource_table_path: Optional[Path],
) -> None:
    """Inventory every scene-map address before any family is reviewed.

    This is deliberately an audit artifact, not an extraction allowlist. A map
    becomes production-visible only after its scene IDs, texture, bundle hashes,
    dimensions, and region transform are copied into the reviewed manifest.
    """
    if not is_safe_relative_identity(build, 128):
        raise SystemExit("build must be a safe exact client-build identity")
    if (scene_table_path is None) != (scene_resource_table_path is None):
        raise SystemExit("--scene-table and --scene-resource-table must be supplied together")
    catalog_path = container / "m0.pkg"
    if not catalog_path.is_file():
        raise SystemExit(f"address catalog is missing: {catalog_path}")
    rows = read_scene_map_address_rows(catalog_path.read_bytes())
    families: dict[str, dict] = {}
    for address, bundle_hash in rows.items():
        parts = address.split("/")
        if len(parts) < 5:
            continue
        family = parts[3]
        entry = families.setdefault(family, {
            "family": family,
            "scene_ids": [],
            "scene_resource_ids": [],
            "texture_candidates": [],
            "region_data": None,
            "auxiliary_assets": [],
        })
        basename = parts[-1]
        record = {"address": address, "bundle_hash": bundle_hash}
        if basename.endswith("_region_data"):
            if entry["region_data"] is not None:
                raise SystemExit(f"scene-map family {family} has multiple region_data addresses")
            entry["region_data"] = record
        elif is_scene_map_texture_candidate(address, basename):
            entry["texture_candidates"].append(record)
        else:
            entry["auxiliary_assets"].append(record)

    table_provenance = None
    if scene_table_path is not None and scene_resource_table_path is not None:
        scene_bytes = bounded_table_bytes(scene_table_path, "SceneTable")
        resource_bytes = bounded_table_bytes(scene_resource_table_path, "SceneResourceTable")
        scenes = require_table_rows(json.loads(scene_bytes), "SceneTable")
        resources = require_table_rows(json.loads(resource_bytes), "SceneResourceTable")
        resource_families: dict[int, str] = {}
        for key, row in resources.items():
            if not isinstance(row, dict) or not isinstance(row.get("SceneFile"), str):
                continue
            resource_id = exact_integer(row.get("Id", key))
            family = row["SceneFile"].replace("\\", "/").rstrip("/").split("/")[-1]
            if resource_id is not None and family:
                resource_families[resource_id] = family
        compact_families: dict[str, list[str]] = {}
        for family in families:
            compact_families.setdefault(compact_scene_family(family), []).append(family)
        for key, row in scenes.items():
            if not isinstance(row, dict):
                continue
            scene_id = exact_integer(row.get("Id", key))
            resource_id = exact_integer(row.get("SceneResourceId"))
            source_family = resource_families.get(resource_id) if resource_id is not None else None
            if scene_id is None or resource_id is None or source_family is None:
                continue
            family = source_family if source_family in families else None
            if family is None:
                matches = compact_families.get(compact_scene_family(source_family), [])
                family = matches[0] if len(matches) == 1 else None
            if family is None:
                continue
            families[family]["scene_ids"].append(scene_id)
            families[family]["scene_resource_ids"].append(resource_id)
        table_provenance = {
            "scene_table": {
                "file": scene_table_path.name,
                "sha256": hashlib.sha256(scene_bytes).hexdigest(),
            },
            "scene_resource_table": {
                "file": scene_resource_table_path.name,
                "sha256": hashlib.sha256(resource_bytes).hexdigest(),
            },
        }

    values = []
    for entry in families.values():
        entry["scene_ids"] = sorted(set(entry["scene_ids"]))
        entry["scene_resource_ids"] = sorted(set(entry["scene_resource_ids"]))
        entry["texture_candidates"].sort(key=lambda value: value["address"])
        entry["auxiliary_assets"].sort(key=lambda value: value["address"])
        entry["review_ready"] = bool(
            entry["scene_ids"] and entry["region_data"] and len(entry["texture_candidates"]) == 1
        )
        values.append(entry)
    values.sort(key=lambda value: value["family"])
    inventory = {
        "schema_version": 1,
        "compiler_version": COMPILER_VERSION,
        "game_build": build,
        "catalog": {
            "file": catalog_path.name,
            "bytes": catalog_path.stat().st_size,
            "scene_map_address_count": len(rows),
        },
        "table_provenance": table_provenance,
        "summary": {
            "map_family_count": len(values),
            "families_with_region_data": sum(value["region_data"] is not None for value in values),
            "families_with_scene_ids": sum(bool(value["scene_ids"]) for value in values),
            "review_ready_candidates": sum(value["review_ready"] for value in values),
        },
        "families": values,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    print(
        f"inventoried {len(values)} map families and {len(rows)} exact addresses "
        f"for {build} -> {output}"
    )


def read_scene_map_address_rows(data: bytes) -> dict[str, int]:
    pattern = re.compile(
        rb"address:(ui/textures/scenemaps/[A-Za-z0-9._/-]+) "
        rb"->>>> hash:\d+ ->>>> bundleHash:(\d+)"
    )
    observed: dict[str, set[int]] = {}
    for address, bundle_hash in pattern.findall(data):
        observed.setdefault(address.decode("ascii"), set()).add(int(bundle_hash))
    ambiguous = {address: hashes for address, hashes in observed.items() if len(hashes) != 1}
    if ambiguous:
        raise SystemExit(f"scene-map addresses resolved to multiple bundle hashes: {ambiguous!r}")
    return {address: next(iter(hashes)) for address, hashes in observed.items()}


def is_scene_map_texture_candidate(address: str, basename: str) -> bool:
    lowered = address.lower()
    return not (
        "/regions/" in lowered
        or "gray_mask" in basename.lower()
        or basename.lower() in {"minimap_cloud", "world_map_cloud"}
    )


def compact_scene_family(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", value.lower())


def bounded_table_bytes(path: Path, label: str) -> bytes:
    if not path.is_file():
        raise SystemExit(f"{label} is missing: {path}")
    if path.stat().st_size > 64 * 1024 * 1024:
        raise SystemExit(f"{label} exceeds 64 MiB")
    return path.read_bytes()


def require_table_rows(value: object, label: str) -> dict:
    if not isinstance(value, dict):
        raise SystemExit(f"{label} must be a JSON object keyed by row ID")
    return value


def exact_integer(value: object) -> Optional[int]:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, str) and re.fullmatch(r"-?\d+", value):
        return int(value)
    return None


def run_self_check() -> None:
    """Exercise packaged imports and the binary parser without reading game files."""
    fixture = bytearray()
    fixture.extend(struct.pack("<iii", 1, 2, 3))
    fixture.extend(b"\0" * 8)
    fixture.extend(struct.pack("<I", 4))
    fixture.extend(struct.pack("<H", 0))
    fixture.extend(struct.pack("<i", 1))
    fixture.extend(struct.pack("<IBHii", 1234, 0, 7, 89, 144))
    fixture.extend(struct.pack("<i", 0))
    expected = [(1234, 7, 89, 144)]
    observed = read_meta_entries(bytes(fixture))
    if observed != expected:
        raise SystemExit(f"self-check meta parser mismatch: {observed!r}")
    if not is_safe_relative_identity("global/steam-24687926", 128):
        raise SystemExit("self-check rejected a valid client-build identity")
    for unsafe in ("/absolute", "../escape", "global//build", "global/./build"):
        if is_safe_relative_identity(unsafe, 128):
            raise SystemExit(f"self-check accepted unsafe build identity: {unsafe}")
    address_fixture = (
        b"address:ui/textures/scenemaps/example/example_example ->>>> hash:1 ->>>> bundleHash:22\n"
        b"address:ui/textures/scenemaps/example/example_region_data ->>>> hash:2 ->>>> bundleHash:23\n"
    )
    if read_scene_map_address_rows(address_fixture) != {
        "ui/textures/scenemaps/example/example_example": 22,
        "ui/textures/scenemaps/example/example_region_data": 23,
    }:
        raise SystemExit("self-check scene-map address inventory mismatch")
    if compact_scene_family("cty001_new") != compact_scene_family("cty001new"):
        raise SystemExit("self-check compact scene-family matching mismatch")
    unitypy_version = getattr(UnityPy, "__version__", "unknown")
    print(
        "self-check passed: "
        f"compiler={COMPILER_VERSION} UnityPy={unitypy_version} Pillow={pillow_version}"
    )


def is_safe_relative_identity(value: str, maximum_length: int) -> bool:
    if not re.fullmatch(rf"[A-Za-z0-9._/-]{{1,{maximum_length}}}", value):
        return False
    if value.startswith("/") or value.endswith("/"):
        return False
    return all(part not in ("", ".", "..") for part in value.split("/"))


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
