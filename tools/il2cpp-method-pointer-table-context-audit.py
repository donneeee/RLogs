#!/usr/bin/env python3
"""Prove whether selected IL2CPP pointers sit in dump declaration order.

The tool builds a small preferred-image pointer sequence from consecutive
dump.cs method declarations and searches section-backed PE bytes for that exact
sequence. A match is registration/table evidence, not evidence that a combat
path invokes any selected getter.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import mmap
import re
import struct
from pathlib import Path

import pefile


DUMP_RVA_RE = re.compile(r"^\s*// RVA: 0x([0-9A-Fa-f]+)\b")
DUMP_NAMESPACE_RE = re.compile(r"^\s*// Namespace:\s*(.*)\s*$")
DUMP_TYPE_RE = re.compile(
    r"^\s*(?:(?:public|private|protected|internal|static|abstract|sealed|partial)\s+)*"
    r"(?:class|struct|interface)\s+([^\s:<{]+)"
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_target(raw: str) -> tuple[int, str]:
    label, separator, value = raw.rpartition("=")
    if not separator:
        label = raw
        value = raw
    try:
        rva = int(value, 0)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"invalid target RVA {value!r}") from error
    if rva <= 0:
        raise argparse.ArgumentTypeError("target RVAs must be positive")
    return rva, label.strip()


def peak_working_set_bytes() -> int | None:
    if not hasattr(ctypes, "windll"):
        return None

    class Counters(ctypes.Structure):
        _fields_ = [
            ("cb", ctypes.c_ulong),
            ("page_fault_count", ctypes.c_ulong),
            ("peak_working_set_size", ctypes.c_size_t),
            ("working_set_size", ctypes.c_size_t),
            ("quota_peak_paged_pool_usage", ctypes.c_size_t),
            ("quota_paged_pool_usage", ctypes.c_size_t),
            ("quota_peak_non_paged_pool_usage", ctypes.c_size_t),
            ("quota_non_paged_pool_usage", ctypes.c_size_t),
            ("pagefile_usage", ctypes.c_size_t),
            ("peak_pagefile_usage", ctypes.c_size_t),
        ]

    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    ok = ctypes.windll.psapi.GetProcessMemoryInfo(
        ctypes.windll.kernel32.GetCurrentProcess(),
        ctypes.byref(counters),
        counters.cb,
    )
    return int(counters.peak_working_set_size) if ok else None


def read_dump_declarations(path: Path) -> list[dict[str, object]]:
    methods: list[dict[str, object]] = []
    namespace = ""
    type_name = "<unknown-type>"
    pending_rva: int | None = None
    with path.open("r", encoding="utf-8", errors="replace") as source:
        for line in source:
            namespace_match = DUMP_NAMESPACE_RE.match(line)
            if namespace_match:
                namespace = namespace_match.group(1).strip()
                continue
            type_match = DUMP_TYPE_RE.match(line)
            if type_match:
                type_name = type_match.group(1)
                continue
            rva_match = DUMP_RVA_RE.match(line)
            if rva_match:
                pending_rva = int(rva_match.group(1), 16)
                continue
            if pending_rva is None:
                continue
            declaration = line.strip()
            if not declaration or declaration.startswith("//"):
                continue
            if "(" in declaration and ")" in declaration:
                qualified_type = f"{namespace}.{type_name}" if namespace else type_name
                methods.append(
                    {
                        "declaration_index": len(methods),
                        "rva": pending_rva,
                        "name": f"{qualified_type}.{declaration.rstrip(' {')}",
                    }
                )
            pending_rva = None
    return methods


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--context-before", type=int, default=8)
    parser.add_argument("--context-after", type=int, default=12)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite {args.output}")
    if args.context_before < 0 or args.context_after < 0:
        raise SystemExit("context bounds must be non-negative")

    methods = read_dump_declarations(args.dump)
    target_labels = {rva: label for rva, label in args.target}
    target_indices: dict[int, list[int]] = {rva: [] for rva in target_labels}
    for index, method in enumerate(methods):
        rva = int(method["rva"])
        if rva in target_indices:
            target_indices[rva].append(index)
    missing = [rva for rva, indices in target_indices.items() if not indices]
    ambiguous = [rva for rva, indices in target_indices.items() if len(indices) != 1]
    if missing or ambiguous:
        raise SystemExit(
            f"targets must have one declaration each; missing={missing}, ambiguous={ambiguous}"
        )

    selected_indices = sorted(indices[0] for indices in target_indices.values())
    first = max(0, selected_indices[0] - args.context_before)
    last = min(len(methods), selected_indices[-1] + args.context_after + 1)
    context = methods[first:last]

    pe = pefile.PE(str(args.binary), fast_load=True)
    image_base = int(pe.OPTIONAL_HEADER.ImageBase)
    needle = b"".join(struct.pack("<Q", image_base + int(row["rva"])) for row in context)
    sections = [
        {
            "name": section.Name.rstrip(b"\0").decode("ascii", errors="replace"),
            "file_start": int(section.PointerToRawData),
            "file_end": int(section.PointerToRawData) + int(section.SizeOfRawData),
            "rva_start": int(section.VirtualAddress),
            "executable": bool(section.Characteristics & 0x20000000),
            "writable": bool(section.Characteristics & 0x80000000),
        }
        for section in pe.sections
    ]
    matches: list[dict[str, object]] = []
    with args.binary.open("rb") as source:
        mapped = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ)
        try:
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
                            "file_offset": file_offset,
                            "sequence_start_rva": int(section["rva_start"])
                            + file_offset
                            - int(section["file_start"]),
                            "section": section["name"],
                            "section_executable": section["executable"],
                            "section_writable": section["writable"],
                        }
                    )
                cursor = file_offset + 1
        finally:
            mapped.close()

    target_rows = []
    for rva, label in sorted(target_labels.items()):
        declaration_index = target_indices[rva][0]
        target_rows.append(
            {
                "label": label,
                "rva": rva,
                "declaration_index": declaration_index,
                "context_index": declaration_index - first,
                "names": [methods[declaration_index]["name"]],
            }
        )

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-method-pointer-table-context-audit.py",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "bytes": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
            "preferred_image_base": image_base,
        },
        "dump": {
            "path": str(args.dump.resolve()),
            "bytes": args.dump.stat().st_size,
            "sha256": sha256(args.dump),
            "method_declarations": len(methods),
        },
        "targets": target_rows,
        "declaration_context": context,
        "sequence": {
            "first_declaration_index": first,
            "last_declaration_index_inclusive": last - 1,
            "entries": len(context),
            "bytes": len(needle),
            "matches": matches,
        },
        "resource_bounds": {
            "binary_memory_mapped": True,
            "needle_bytes": len(needle),
            "measured_process_peak_working_set_bytes": peak_working_set_bytes(),
        },
        "conclusion": {
            "exact_dump_declaration_pointer_sequence_found": len(matches) > 0,
            "unique_exact_sequence_match": len(matches) == 1,
            "selected_pointers_are_generic_method_registration_sequence": len(matches) == 1,
            "selected_pointer_sequence_is_combat_consumer_proof": False,
            "runtime_indexed_combat_dispatch_proven": False,
            "provider_rdps_credit_allowed": False,
        },
        "policy": {
            "exact_build_binary_and_dump_required": True,
            "preferred_image_pointer_sequence_is_registration_evidence": True,
            "registration_sequence_is_invocation_evidence": False,
            "formula_authority": False,
            "runtime_authority": False,
            "provider_rdps_credit_allowed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "method_declarations": len(methods),
                "sequence_entries": len(context),
                "exact_sequence_matches": len(matches),
                "matches": matches,
                "measured_process_peak_working_set_bytes": report["resource_bounds"][
                    "measured_process_peak_working_set_bytes"
                ],
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
