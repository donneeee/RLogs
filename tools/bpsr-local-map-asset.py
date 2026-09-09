#!/usr/bin/env python3
"""Compile a reviewed BPSR map texture into rLogs' local-only asset namespace."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import tempfile
from pathlib import Path
from typing import Optional

import UnityPy  # type: ignore
from PIL import __version__ as pillow_version  # type: ignore

DEFAULT_ADDRESS = "ui/textures/map/dungeon_map_bg"
DEFAULT_OBJECT_NAME = "dungeon_map_bg"
COMPILER_VERSION = "4"
MAXIMUM_LOCALIZATION_PAYLOAD_BYTES = 64 * 1024 * 1024
MAXIMUM_LOCALIZATION_ENTRIES = 1_000_000
MAXIMUM_META_ENTRIES = 1_000_000


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", action="store_true")
    parser.add_argument("--self-check", action="store_true")
    parser.add_argument("--reviewed-manifest", type=Path)
    parser.add_argument("--localization-manifest", type=Path)
    parser.add_argument("--inventory-output", type=Path)
    parser.add_argument("--inventory-input", type=Path)
    parser.add_argument("--candidate-manifest-output", type=Path)
    parser.add_argument("--scene-table", type=Path)
    parser.add_argument("--scene-resource-table", type=Path)
    parser.add_argument("--container", type=Path)
    parser.add_argument("--runtime-root", type=Path)
    parser.add_argument("--build")
    parser.add_argument("--reviewed-build")
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
    if args.localization_manifest is not None and args.reviewed_manifest is None:
        parser.error("--localization-manifest requires --reviewed-manifest")
    if args.reviewed_manifest is not None:
        compile_reviewed_manifest(
            args.container,
            args.runtime_root,
            args.build,
            args.reviewed_build or args.build,
            args.reviewed_manifest,
            args.localization_manifest,
        )
        return
    compile_asset(args)


def compile_asset(
    args: argparse.Namespace,
    expected: Optional[dict] = None,
    address_catalog: Optional[bytes] = None,
    meta_entries: Optional[list[tuple[int, int, int, int]]] = None,
    strict_bundle_hashes: bool = True,
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
    if expected is not None and strict_bundle_hashes and bundle_hash != expected["source_bundle_hash"]:
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
        if expected is not None and strict_bundle_hashes and region_bundle_hash != expected["region_bundle_hash"]:
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


def compile_reviewed_localization(
    container: Path,
    build_root: Path,
    build: str,
    reviewed_build: str,
    manifest_path: Path,
    meta_entries: list[tuple[int, int, int, int, int]],
) -> None:
    if not is_safe_relative_identity(build, 128):
        raise SystemExit("build must be a safe exact client-build identity")
    manifest_bytes = manifest_path.read_bytes()
    if len(manifest_bytes) > 32 * 1024:
        raise SystemExit("reviewed localization manifest exceeds 32 KiB")
    value = json.loads(manifest_bytes)
    if not isinstance(value, dict) or set(value) != {"schema_version", "builds"}:
        raise SystemExit("reviewed localization manifest has an invalid root")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise SystemExit("reviewed localization manifest has an unsupported schema")
    if not isinstance(value["builds"], dict):
        raise SystemExit("reviewed localization manifest has an unsupported schema")
    entries = value["builds"].get(reviewed_build)
    if not isinstance(entries, list) or not entries or len(entries) > 32:
        raise SystemExit("reviewed localization manifest has an invalid entry count")

    output_root = build_root / "localization"
    output_root.mkdir(parents=True, exist_ok=True)
    catalog_entries = []
    seen_languages = set()
    seen_locales = set()
    seen_keys = set()
    required = {
        "language", "locale", "entry_key", "bytes", "sha256", "index_entries",
        "string_entries", "populated_strings",
    }
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != required:
            raise SystemExit("reviewed localization entry has an invalid shape")
        language = entry["language"]
        locale = entry["locale"]
        entry_key = entry["entry_key"]
        if not isinstance(language, str) or not re.fullmatch(r"[a-z]{2,32}", language):
            raise SystemExit("reviewed localization language is invalid")
        if not isinstance(locale, str) or not re.fullmatch(r"[a-z]{2,3}(?:-[A-Z]{2})?", locale):
            raise SystemExit("reviewed localization locale is invalid")
        if not isinstance(entry_key, int) or isinstance(entry_key, bool):
            raise SystemExit("reviewed localization entry key is invalid")
        if entry_key < 0 or entry_key > 0xFFFFFFFF:
            raise SystemExit("reviewed localization entry key is outside uint32")
        if (
            type(entry["bytes"]) is not int
            or entry["bytes"] <= 0
            or entry["bytes"] > MAXIMUM_LOCALIZATION_PAYLOAD_BYTES
            or not isinstance(entry["sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"])
            or any(
                type(entry[field]) is not int
                or entry[field] <= 0
                or entry[field] > MAXIMUM_LOCALIZATION_ENTRIES
                for field in ("index_entries", "string_entries", "populated_strings")
            )
        ):
            raise SystemExit("reviewed localization expectation is invalid")
        if language in seen_languages or locale in seen_locales or entry_key in seen_keys:
            raise SystemExit("reviewed localization manifest contains a duplicate")
        seen_languages.add(language)
        seen_locales.add(locale)
        seen_keys.add(entry_key)
        matches = [row for row in meta_entries if row[0] == entry_key]
        if len(matches) != 1:
            raise SystemExit(
                f"expected one exact localization entry {entry_key}, observed {len(matches)}"
            )
        _, entry_type, package_index, entry_offset, length = matches[0]
        if entry_type != 1:
            raise SystemExit(f"localization entry {entry_key} has unexpected type {entry_type}")
        payload, source_package = read_container_payload(
            container, package_index, entry_offset, length
        )
        summary = validate_localization_payload(payload)
        digest = hashlib.sha256(payload).hexdigest()
        if build == reviewed_build:
            expected = {
                "bytes": len(payload),
                "sha256": digest,
                "index_entries": summary["index_entries"],
                "string_entries": summary["string_entries"],
                "populated_strings": summary["populated_strings"],
            }
            for field, observed in expected.items():
                if entry[field] != observed:
                    raise SystemExit(
                        f"reviewed localization {locale} {field} changed: "
                        f"expected {entry[field]!r}, observed {observed!r}"
                    )
        asset = f"{locale}.bin"
        (output_root / asset).write_bytes(payload)
        catalog_entries.append({
            "language": language,
            "locale": locale,
            "asset": asset,
            "source_entry_key": entry_key,
            "source_entry_type": entry_type,
            "source_package": source_package.name,
            "bytes": len(payload),
            "sha256": digest,
            **summary,
        })

    catalog_entries.sort(key=lambda row: row["locale"])
    catalog = {
        "schema_version": 1,
        "game_build": build,
        "presentation_only": True,
        "mechanics_authority": False,
        "upload_allowed": False,
        "entries": catalog_entries,
    }
    (output_root / "catalog.v1.json").write_text(
        json.dumps(catalog, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def compile_reviewed_manifest(
    container: Path,
    runtime_root: Path,
    build: str,
    reviewed_build: str,
    manifest_path: Path,
    localization_manifest_path: Optional[Path] = None,
) -> None:
    if not is_safe_relative_identity(build, 128):
        raise SystemExit("build must be a safe exact client-build identity")
    if not is_safe_relative_identity(reviewed_build, 128):
        raise SystemExit("reviewed build must be a safe exact client-build identity")
    if manifest_path.stat().st_size > 128 * 1024:
        raise SystemExit("reviewed map manifest exceeds 128 KiB")
    value = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != {"schema_version", "builds"}:
        raise SystemExit("reviewed map manifest has an invalid root")
    if value["schema_version"] != 1 or not isinstance(value["builds"], dict):
        raise SystemExit("reviewed map manifest has an unsupported schema")
    entries = value["builds"].get(reviewed_build)
    if not isinstance(entries, list) or not entries:
        raise SystemExit(f"no reviewed local map assets match reviewed build {reviewed_build}")
    required = {
        "address", "object_name", "asset", "region_address", "source_bundle_hash",
        "region_bundle_hash", "width", "height", "origin_x", "origin_z", "span_x",
        "span_z", "scene_ids",
    }
    runtime_root.mkdir(parents=True, exist_ok=True)
    target = runtime_root / build
    backup = runtime_root / f".{build}.previous"
    if backup.exists():
        if target.exists():
            shutil.rmtree(backup)
        else:
            backup.replace(target)
    staging_root = Path(tempfile.mkdtemp(prefix=".rlogs-map-stage-", dir=runtime_root))
    try:
        address_catalog = (container / "m0.pkg").read_bytes()
        all_meta_entries = read_all_meta_entries((container / "meta.pkg").read_bytes())
        meta_entries = [
            (key, package_index, entry_offset, length)
            for key, entry_type, package_index, entry_offset, length in all_meta_entries
            if entry_type == 0
        ]
        for entry in entries:
            if not isinstance(entry, dict) or set(entry) != required:
                raise SystemExit("reviewed map manifest entry has invalid fields")
            if not isinstance(entry["scene_ids"], list) or not entry["scene_ids"]:
                raise SystemExit("reviewed map manifest entry has no scene IDs")
            compile_asset(
                argparse.Namespace(
                    container=container,
                    runtime_root=staging_root,
                    build=build,
                    address=entry["address"],
                    object_name=entry["object_name"],
                    asset=entry["asset"],
                    region_address=entry["region_address"],
                ),
                entry,
                address_catalog,
                meta_entries,
                strict_bundle_hashes=build == reviewed_build,
            )
        if localization_manifest_path is not None:
            compile_reviewed_localization(
                container,
                staging_root / build,
                build,
                reviewed_build,
                localization_manifest_path,
                all_meta_entries,
            )
        staged = staging_root / build
        if backup.exists():
            shutil.rmtree(backup)
        if target.exists():
            target.replace(backup)
        try:
            staged.replace(target)
        except BaseException:
            if backup.exists() and not target.exists():
                backup.replace(target)
            raise
        if backup.exists():
            shutil.rmtree(backup)
    finally:
        if staging_root.exists():
            shutil.rmtree(staging_root)
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


def read_container_payload(
    container: Path,
    package_index: int,
    entry_offset: int,
    length: int,
) -> tuple[bytes, Path]:
    if (
        not isinstance(package_index, int)
        or isinstance(package_index, bool)
        or package_index < 0
        or package_index > 0xFFFF
    ):
        raise SystemExit("container package index is invalid")
    if (
        not isinstance(entry_offset, int)
        or isinstance(entry_offset, bool)
        or entry_offset < 0
    ):
        raise SystemExit("container entry offset is invalid")
    if (
        not isinstance(length, int)
        or isinstance(length, bool)
        or length <= 0
        or length > MAXIMUM_LOCALIZATION_PAYLOAD_BYTES
    ):
        raise SystemExit("container entry length is invalid")

    package = container / f"m{package_index}.pkg"
    if not package.is_file():
        raise SystemExit(f"container package is missing: {package.name}")
    package_bytes = package.stat().st_size
    if entry_offset > package_bytes or length > package_bytes - entry_offset:
        raise SystemExit(f"container entry exceeds {package.name}")
    with package.open("rb") as handle:
        handle.seek(entry_offset)
        payload = handle.read(length)
    if len(payload) != length:
        raise SystemExit(f"container entry ended early in {package.name}")
    return payload, package


def read_7bit_uint32(data: bytes, offset: int, end: int) -> tuple[int, int]:
    value = 0
    for index in range(5):
        if offset >= end:
            raise SystemExit("localization string length ended early")
        byte = data[offset]
        offset += 1
        if index == 4 and byte > 0x0F:
            raise SystemExit("localization string length exceeds uint32")
        value |= (byte & 0x7F) << (index * 7)
        if byte < 0x80:
            if index > 0 and value < 1 << (index * 7):
                raise SystemExit("localization string length is not canonical")
            return value, offset
    raise SystemExit("localization string length exceeds five bytes")


def validate_localization_payload(payload: bytes) -> dict:
    if not payload or len(payload) > MAXIMUM_LOCALIZATION_PAYLOAD_BYTES:
        raise SystemExit("localization payload has an invalid size")
    if len(payload) < 16:
        raise SystemExit("localization payload ended before its header and trailer")

    (index_entries,) = struct.unpack_from("<I", payload, 0)
    if index_entries == 0 or index_entries > MAXIMUM_LOCALIZATION_ENTRIES:
        raise SystemExit("localization payload has an invalid index entry count")
    index_end = 4 + index_entries * 8
    if index_end + 12 > len(payload):
        raise SystemExit("localization payload index ended early")

    referenced_strings = []
    previous_localization_id = None
    for offset in range(4, index_end, 8):
        localization_id, string_index = struct.unpack_from("<II", payload, offset)
        if (
            previous_localization_id is not None
            and localization_id <= previous_localization_id
        ):
            raise SystemExit("localization payload IDs are not strictly increasing")
        previous_localization_id = localization_id
        referenced_strings.append(string_index)

    (string_entries,) = struct.unpack_from("<I", payload, index_end)
    if string_entries == 0 or string_entries > MAXIMUM_LOCALIZATION_ENTRIES:
        raise SystemExit("localization payload has an invalid string entry count")
    missing_text_references = sum(
        1 for string_index in referenced_strings if string_index >= string_entries
    )
    if missing_text_references:
        raise SystemExit(
            f"localization payload has {missing_text_references} out-of-range text references"
        )

    cursor = index_end + 4
    strings_end = len(payload) - 8
    populated_strings = 0
    for _index in range(string_entries):
        length, cursor = read_7bit_uint32(payload, cursor, strings_end)
        if length > strings_end - cursor:
            raise SystemExit("localization string exceeds its payload")
        try:
            payload[cursor : cursor + length].decode("utf-8")
        except UnicodeDecodeError as error:
            raise SystemExit(f"localization string is not valid UTF-8: {error}") from error
        populated_strings += int(length > 0)
        cursor += length
    if cursor != strings_end or payload[strings_end:] != b"\0" * 8:
        raise SystemExit("localization payload has an invalid trailer")

    return {
        "index_entries": index_entries,
        "string_entries": string_entries,
        "populated_strings": populated_strings,
        "missing_text_references": 0,
    }


def run_self_check() -> None:
    """Exercise packaged imports and the binary parser without reading game files."""
    fixture = bytearray()
    fixture.extend(struct.pack("<iii", 1, 2, 3))
    fixture.extend(b"\0" * 8)
    fixture.extend(struct.pack("<I", 4))
    fixture.extend(struct.pack("<H", 0))
    fixture.extend(struct.pack("<i", 1))
    fixture.extend(struct.pack("<IBHii", 1234, 0, 7, 89, 144))
    fixture.extend(struct.pack("<i", 1))
    fixture.extend(struct.pack("<IBHii", 5678, 1, 0, 233, 377))
    expected = [(1234, 7, 89, 144)]
    observed = read_meta_entries(bytes(fixture))
    if observed != expected:
        raise SystemExit(f"self-check meta parser mismatch: {observed!r}")
    expected_all = [(1234, 0, 7, 89, 144), (5678, 1, 0, 233, 377)]
    observed_all = read_all_meta_entries(bytes(fixture))
    if observed_all != expected_all:
        raise SystemExit(f"self-check full meta parser mismatch: {observed_all!r}")
    localization_fixture = (
        struct.pack("<I", 3)
        + struct.pack("<IIIIII", 100, 0, 200, 1, 300, 1)
        + struct.pack("<I", 2)
        + b"\x00\x03\xe2\x98\x83"
        + b"\0" * 8
    )
    localization_summary = validate_localization_payload(localization_fixture)
    if localization_summary != {
        "index_entries": 3,
        "string_entries": 2,
        "populated_strings": 1,
        "missing_text_references": 0,
    }:
        raise SystemExit(
            f"self-check localization parser mismatch: {localization_summary!r}"
        )
    malformed_localization_fixtures = {
        "truncated trailer": localization_fixture[:-1],
        "nonzero trailer": localization_fixture[:-1] + b"x",
        "out-of-order IDs": (
            struct.pack("<I", 2)
            + struct.pack("<IIII", 200, 0, 100, 0)
            + struct.pack("<I", 1)
            + b"\x00"
            + b"\0" * 8
        ),
        "out-of-range string reference": (
            struct.pack("<I", 1)
            + struct.pack("<II", 100, 1)
            + struct.pack("<I", 1)
            + b"\x00"
            + b"\0" * 8
        ),
        "noncanonical string length": (
            struct.pack("<I", 1)
            + struct.pack("<II", 100, 0)
            + struct.pack("<I", 1)
            + b"\x80\x00"
            + b"\0" * 8
        ),
        "invalid UTF-8": (
            struct.pack("<I", 1)
            + struct.pack("<II", 100, 0)
            + struct.pack("<I", 1)
            + b"\x01\xff"
            + b"\0" * 8
        ),
    }
    for label, malformed in malformed_localization_fixtures.items():
        try:
            validate_localization_payload(malformed)
        except SystemExit:
            pass
        else:
            raise SystemExit(f"self-check accepted localization payload with {label}")
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


def read_all_meta_entries(data: bytes) -> list[tuple[int, int, int, int, int]]:
    offset = 0

    def take(fmt: str) -> tuple[int, ...]:
        nonlocal offset
        size = struct.calcsize(fmt)
        if offset + size > len(data):
            raise SystemExit("meta.pkg ended early")
        values = struct.unpack_from(fmt, data, offset)
        offset += size
        return values

    def skip(size: int) -> None:
        nonlocal offset
        if size < 0 or offset + size > len(data):
            raise SystemExit("meta.pkg ended early")
        offset += size

    take("<iii")
    skip(8)
    take("<I")
    (header_count,) = take("<H")
    skip(16 * header_count)
    entries = []
    for _section in range(2):
        (count,) = take("<i")
        if count < 0 or count > MAXIMUM_META_ENTRIES:
            raise SystemExit("meta.pkg has an invalid entry count")
        for _index in range(count):
            key, entry_type, package_index, entry_offset, length = take("<IBHii")
            if entry_type not in (0, 1):
                raise SystemExit(f"meta.pkg has unsupported entry type {entry_type}")
            if entry_offset < 0 or length <= 0:
                raise SystemExit("meta.pkg has an invalid entry extent")
            entries.append((key, entry_type, package_index, entry_offset, length))
    return entries


def read_meta_entries(data: bytes) -> list[tuple[int, int, int, int]]:
    return [
        (key, package_index, entry_offset, length)
        for key, entry_type, package_index, entry_offset, length in read_all_meta_entries(data)
        if entry_type == 0
    ]


if __name__ == "__main__":
    main()
