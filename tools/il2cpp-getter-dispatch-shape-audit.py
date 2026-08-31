#!/usr/bin/env python3
"""Retain exact IL2CPP getter bodies and their native dispatch shape.

This bounded extractor is deliberately narrower than a decompiler. It maps
selected method RVAs through Il2CppDumper's dump.cs, decodes only each selected
method interval, and records immediate argument writes plus direct calls/jumps.
An immediate passed to a shared ReadProxy helper is a table-column offset; it
must not be reported as an object field offset or a damage formula consumer.
"""

from __future__ import annotations

import argparse
import bisect
import ctypes
import hashlib
import json
import re
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_64, Cs
from capstone.x86_const import X86_OP_IMM


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


def read_dump_methods(path: Path) -> list[dict[str, object]]:
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
                        "rva": pending_rva,
                        "name": f"{qualified_type}.{declaration.rstrip(' {')}",
                    }
                )
            pending_rva = None
    methods.sort(key=lambda row: (int(row["rva"]), str(row["name"])))
    return methods


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--maximum-method-bytes", type=int, default=4096)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite {args.output}")
    if args.maximum_method_bytes <= 0:
        raise SystemExit("--maximum-method-bytes must be positive")

    methods = read_dump_methods(args.dump)
    by_rva: dict[int, list[str]] = {}
    for method in methods:
        by_rva.setdefault(int(method["rva"]), []).append(str(method["name"]))
    starts = sorted(by_rva)

    pe = pefile.PE(str(args.binary), fast_load=True)
    decoder = Cs(CS_ARCH_X86, CS_MODE_64)
    decoder.detail = True
    reports: list[dict[str, object]] = []
    maximum_decoded_bytes = 0
    for target_rva, label in sorted(args.target):
        index = bisect.bisect_left(starts, target_rva)
        if index >= len(starts) or starts[index] != target_rva:
            raise SystemExit(f"target RVA 0x{target_rva:x} is absent from dump method index")
        next_index = index + 1
        while next_index < len(starts) and starts[next_index] == target_rva:
            next_index += 1
        if next_index >= len(starts):
            raise SystemExit(f"target RVA 0x{target_rva:x} has no bounded next method")
        method_end = starts[next_index]
        byte_length = method_end - target_rva
        if byte_length <= 0 or byte_length > args.maximum_method_bytes:
            raise SystemExit(
                f"target RVA 0x{target_rva:x} interval {byte_length} exceeds bounds"
            )
        data = pe.get_data(target_rva, byte_length)
        if len(data) != byte_length:
            raise SystemExit(f"short PE read for target RVA 0x{target_rva:x}")
        maximum_decoded_bytes = max(maximum_decoded_bytes, len(data))
        instructions = list(decoder.disasm(data, target_rva))
        rows = [
            {
                "rva": int(instruction.address),
                "bytes_hex": bytes(instruction.bytes).hex(),
                "mnemonic": str(instruction.mnemonic),
                "operands": str(instruction.op_str),
            }
            for instruction in instructions
        ]
        direct_transfers = []
        edx_immediates = []
        for instruction in instructions:
            if (
                instruction.mnemonic in {"call", "jmp"}
                and instruction.operands
                and instruction.operands[0].type == X86_OP_IMM
            ):
                destination = int(instruction.operands[0].imm)
                direct_transfers.append(
                    {
                        "instruction_rva": int(instruction.address),
                        "kind": str(instruction.mnemonic),
                        "destination_rva": destination,
                        "destination_names": sorted(by_rva.get(destination, [])),
                    }
                )
            if instruction.mnemonic == "mov" and instruction.op_str.startswith("edx, 0x"):
                edx_immediates.append(int(instruction.op_str.split("0x", 1)[1], 16))
        reports.append(
            {
                "label": label,
                "method_rva": target_rva,
                "method_names": sorted(by_rva[target_rva]),
                "method_end_rva": method_end,
                "method_bytes": byte_length,
                "edx_immediates": edx_immediates,
                "direct_transfers": direct_transfers,
                "instructions": rows,
            }
        )

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-getter-dispatch-shape-audit.py",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "bytes": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
        },
        "dump": {
            "path": str(args.dump.resolve()),
            "bytes": args.dump.stat().st_size,
            "sha256": sha256(args.dump),
            "method_entries": len(methods),
        },
        "getters": reports,
        "resource_bounds": {
            "selected_methods_only": True,
            "maximum_method_bytes": args.maximum_method_bytes,
            "maximum_decoded_bytes": maximum_decoded_bytes,
            "measured_process_peak_working_set_bytes": peak_working_set_bytes(),
        },
        "policy": {
            "exact_build_binary_and_dump_required": True,
            "immediate_edx_value_is_table_column_offset": True,
            "immediate_edx_value_is_object_field_offset": False,
            "getter_dispatch_is_formula_consumer": False,
            "indirect_runtime_consumers_enumerated": False,
            "provider_rdps_credit_allowed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "getters": len(reports),
                "column_offsets": {
                    row["label"]: row["edx_immediates"] for row in reports
                },
                "maximum_decoded_bytes": maximum_decoded_bytes,
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
