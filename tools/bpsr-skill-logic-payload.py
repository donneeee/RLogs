#!/usr/bin/env python3
"""Extract the exact current-build skill-logic TextAsset without widening scope."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

import UnityPy  # type: ignore


SCHEMA_VERSION = 1
GENERATED_BY = "tools/bpsr-skill-logic-payload.py"
EXPECTED_BUILD = "24687926"
EXPECTED_BUNDLE_SHA256 = (
    "54309b4ce21008c8acf854eda1028ead3e3d3136341520304aa76cc51aa3a6bc"
)
EXPECTED_TEXT_ASSET = "logic_skill_bullet"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def artifact(path: Path, data: bytes) -> dict[str, object]:
    return {"file": str(path), "bytes": len(data), "sha256": sha256(data)}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    analyze = subparsers.add_parser("analyze")
    analyze.add_argument("--build", required=True)
    analyze.add_argument("--bundle", type=Path, required=True)
    analyze.add_argument("--output-payload", type=Path, required=True)
    analyze.add_argument("--output", type=Path, required=True)
    verify = subparsers.add_parser("verify")
    verify.add_argument("--input", type=Path, required=True)
    verify.add_argument("--payload", type=Path, required=True)
    return parser.parse_args()


def validate(report: dict[str, object], payload: bytes) -> None:
    authority = report.get("authority")
    text_asset = report.get("text_asset")
    payload_artifact = report.get("payload")
    if not isinstance(authority, dict) or not isinstance(text_asset, dict):
        raise RuntimeError("payload proof is missing authority or TextAsset identity")
    if not isinstance(payload_artifact, dict):
        raise RuntimeError("payload proof is missing payload identity")
    if (
        report.get("schema_version") != SCHEMA_VERSION
        or report.get("generated_by") != GENERATED_BY
        or report.get("build") != EXPECTED_BUILD
        or text_asset.get("name") != EXPECTED_TEXT_ASSET
        or payload_artifact.get("bytes") != len(payload)
        or payload_artifact.get("sha256") != sha256(payload)
        or authority.get("exact_build_skill_logic_payload_extracted") is not True
        or authority.get("stage_logic_payload_decoded") is not False
        or authority.get("packet_owner_stage_to_stage_type_mapping_proven") is not False
        or authority.get("runtime_promotion_allowed") is not False
        or authority.get("provider_rdps_credit_allowed") is not False
    ):
        raise RuntimeError("skill-logic payload proof is not fail-closed exact-build evidence")


def analyze(args: argparse.Namespace) -> None:
    if args.build != EXPECTED_BUILD:
        raise RuntimeError(f"this reviewed extractor supports only build {EXPECTED_BUILD}")
    for output in (args.output_payload, args.output):
        if output.exists():
            raise RuntimeError(f"refusing to overwrite existing output: {output}")
    bundle = args.bundle.read_bytes()
    if sha256(bundle) != EXPECTED_BUNDLE_SHA256 or not bundle.startswith(b"UnityFS"):
        raise RuntimeError("bundle is not the reviewed exact current-build skill-logic carrier")

    environment = UnityPy.load(str(args.bundle))
    objects = list(environment.objects)
    text_objects = [obj for obj in objects if obj.type.name == "TextAsset"]
    if len(text_objects) != 1:
        raise RuntimeError(f"expected one TextAsset, observed {len(text_objects)}")
    text_object = text_objects[0]
    value = text_object.read()
    if value.m_Name != EXPECTED_TEXT_ASSET:
        raise RuntimeError(f"unexpected TextAsset name {value.m_Name!r}")
    if not isinstance(value.m_Script, str):
        raise RuntimeError("UnityPy returned an unknown TextAsset payload representation")
    payload = value.m_Script.encode("utf-8", "surrogateescape")
    args.output_payload.write_bytes(payload)

    report: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "generated_by": GENERATED_BY,
        "game": "blue-protocol-star-resonance",
        "deployment": "global",
        "channel": "steam",
        "build": EXPECTED_BUILD,
        "extractor": {
            "python": sys.version.split()[0],
            "unitypy": getattr(UnityPy, "__version__", "unknown"),
            "binary_text_round_trip": "utf-8-surrogateescape",
        },
        "bundle": artifact(args.bundle, bundle),
        "unity_objects": {
            "total": len(objects),
            "text_assets": len(text_objects),
            "asset_bundles": sum(obj.type.name == "AssetBundle" for obj in objects),
        },
        "text_asset": {
            "name": value.m_Name,
            "path_id": text_object.path_id,
            "serialized_object_bytes": text_object.byte_size,
        },
        "payload": artifact(args.output_payload, payload),
        "authority": {
            "exact_build_skill_logic_payload_extracted": True,
            "stage_logic_payload_decoded": False,
            "packet_owner_stage_to_stage_type_mapping_proven": False,
            "runtime_promotion_allowed": False,
            "provider_rdps_credit_allowed": False,
        },
    }
    validate(report, payload)
    args.output.write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote exact-build skill logic payload proof: {args.output}")
    print(f"payload: {len(payload)} bytes, sha256 {sha256(payload)}")


def verify(args: argparse.Namespace) -> None:
    report = json.loads(args.input.read_text(encoding="utf-8"))
    payload = args.payload.read_bytes()
    validate(report, payload)
    print(f"verified exact-build skill logic payload: {args.input}")


def main() -> None:
    args = parse_args()
    if args.command == "analyze":
        analyze(args)
    else:
        verify(args)


if __name__ == "__main__":
    main()
