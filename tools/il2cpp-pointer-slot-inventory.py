#!/usr/bin/env python3
"""Inventory exact PE slots containing selected IL2CPP method pointers.

This bounded extractor searches section-backed bytes for the preferred-image
absolute virtual address and the 32-bit RVA of each selected method.  A literal
slot is registration/table evidence only; it does not prove that runtime code
loads or calls through that slot.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import mmap
import struct
from pathlib import Path

import pefile


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_target(raw: str) -> tuple[int, str]:
    if "=" in raw:
        label, value = raw.rsplit("=", 1)
    else:
        label, value = raw, raw
    rva = int(value, 0)
    if rva <= 0 or rva > 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("target RVAs must fit a nonzero uint32")
    return rva, label.strip()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite {args.output}")

    targets: dict[int, list[str]] = {}
    for rva, label in args.target:
        targets.setdefault(rva, []).append(label)

    pe = pefile.PE(str(args.binary), fast_load=True)
    image_base = int(pe.OPTIONAL_HEADER.ImageBase)
    sections: list[dict[str, object]] = []
    for section in pe.sections:
        sections.append(
            {
                "name": section.Name.rstrip(b"\0").decode("ascii", errors="replace"),
                "file_start": int(section.PointerToRawData),
                "file_end": int(section.PointerToRawData) + int(section.SizeOfRawData),
                "rva_start": int(section.VirtualAddress),
                "rva_end": int(section.VirtualAddress)
                + max(int(section.Misc_VirtualSize), int(section.SizeOfRawData)),
                "executable": bool(section.Characteristics & 0x20000000),
                "readable": bool(section.Characteristics & 0x40000000),
                "writable": bool(section.Characteristics & 0x80000000),
            }
        )

    matches: list[dict[str, object]] = []
    with args.binary.open("rb") as source:
        mapped = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            for rva, labels in sorted(targets.items()):
                encodings = (
                    ("preferred-image-absolute-va-u64", struct.pack("<Q", image_base + rva)),
                    ("rva-u32", struct.pack("<I", rva)),
                )
                for encoding, needle in encodings:
                    cursor = 0
                    while True:
                        file_offset = mapped.find(needle, cursor)
                        if file_offset < 0:
                            break
                        section = next(
                            (
                                row
                                for row in sections
                                if int(row["file_start"]) <= file_offset
                                and file_offset + len(needle) <= int(row["file_end"])
                            ),
                            None,
                        )
                        if section is not None:
                            matches.append(
                                {
                                    "target_rva": rva,
                                    "target_labels": sorted(labels),
                                    "encoding": encoding,
                                    "file_offset": file_offset,
                                    "slot_rva": int(section["rva_start"])
                                    + file_offset
                                    - int(section["file_start"]),
                                    "section": str(section["name"]),
                                    "section_executable": bool(section["executable"]),
                                    "section_writable": bool(section["writable"]),
                                }
                            )
                        cursor = file_offset + 1
        finally:
            mapped.close()

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-pointer-slot-inventory.py",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "bytes": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
            "preferred_image_base": image_base,
        },
        "targets": [
            {"rva": rva, "labels": sorted(labels)}
            for rva, labels in sorted(targets.items())
        ],
        "sections": [
            {
                "name": row["name"],
                "rva_start": row["rva_start"],
                "rva_end": row["rva_end"],
                "executable": row["executable"],
                "readable": row["readable"],
                "writable": row["writable"],
            }
            for row in sections
        ],
        "matches": sorted(
            matches,
            key=lambda row: (
                int(row["target_rva"]),
                str(row["encoding"]),
                int(row["slot_rva"]),
            ),
        ),
        "summary": {
            "targets": len(targets),
            "preferred_image_absolute_pointer_slots": sum(
                row["encoding"] == "preferred-image-absolute-va-u64" for row in matches
            ),
            "rva_u32_literal_matches": sum(row["encoding"] == "rva-u32" for row in matches),
            "targets_with_preferred_image_absolute_pointer_slots": len(
                {
                    int(row["target_rva"])
                    for row in matches
                    if row["encoding"] == "preferred-image-absolute-va-u64"
                }
            ),
        },
        "policy": {
            "exact_literal_encoding_match": True,
            "literal_slot_is_runtime_reference": False,
            "literal_slot_is_indirect_call": False,
            "rip_relative_or_indexed_consumer_proof_required": True,
            "formula_authority": False,
            "runtime_authority": False,
            "provider_rdps_credit_allowed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
