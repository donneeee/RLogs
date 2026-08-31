#!/usr/bin/env python3
"""Find bounded IL2CPP method candidates that access selected object offsets.

This is an evidence extractor, not a decompiler. It selects methods by exact
native signature fragments from Il2CppDumper's script.json, disassembles only
their bounded address intervals, and retains non-stack memory operands whose
displacement matches a requested field offset. A displacement match alone is
never treated as object identity or formula authority.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_64, Cs
from capstone.x86_const import X86_OP_MEM


ADDRESS_RE = re.compile(r'^\s*"Address":\s*(\d+),?\s*$')
STRING_RE = re.compile(r'^\s*"(Name|Signature)":\s*("(?:\\.|[^"\\])*")\s*,?\s*$')
EXCLUDED_BASES = {"rip", "rsp", "rbp"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_methods(path: Path) -> list[dict[str, object]]:
    methods: list[dict[str, object]] = []
    pending: dict[str, object] = {}
    in_methods = False
    with path.open("r", encoding="utf-8") as source:
        for line in source:
            if not in_methods:
                if '"ScriptMethod"' in line:
                    in_methods = True
                continue
            if line.startswith('  "ScriptMetadata"'):
                break
            address_match = ADDRESS_RE.match(line)
            if address_match:
                pending = {"address": int(address_match.group(1))}
                continue
            string_match = STRING_RE.match(line)
            if not string_match or "address" not in pending:
                continue
            pending[string_match.group(1).lower()] = json.loads(string_match.group(2))
            if "name" in pending and "signature" in pending:
                methods.append(pending)
                pending = {}
    methods.sort(key=lambda row: (int(row["address"]), str(row["name"])))
    return methods


def parse_offset(raw: str) -> int:
    value = int(raw, 0)
    if value < 0:
        raise argparse.ArgumentTypeError("field offsets must be non-negative")
    return value


def instruction_row(instruction: object) -> dict[str, object]:
    return {
        "rva": int(instruction.address),
        "size": int(instruction.size),
        "mnemonic": str(instruction.mnemonic),
        "operands": str(instruction.op_str),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--script-json", required=True, type=Path)
    parser.add_argument("--signature-fragment", action="append", required=True)
    parser.add_argument("--field-offset", action="append", required=True, type=parse_offset)
    parser.add_argument("--maximum-method-bytes", type=int, default=1_048_576)
    parser.add_argument("--context-instructions", type=int, default=5)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    if args.maximum_method_bytes <= 0 or args.context_instructions < 0:
        raise SystemExit("method and context bounds must be non-negative")

    methods = read_methods(args.script_json)
    fragments = [fragment.casefold() for fragment in args.signature_fragment]
    candidates = [
        row
        for row in methods
        if any(fragment in str(row["signature"]).casefold() for fragment in fragments)
    ]
    if not candidates:
        raise SystemExit("no method signatures matched")

    addresses = sorted({int(row["address"]) for row in methods if int(row["address"]) > 0})
    pe = pefile.PE(str(args.binary), fast_load=True)
    pe.parse_data_directories()
    executable_ranges = [
        (
            int(section.VirtualAddress),
            int(section.VirtualAddress) + max(int(section.Misc_VirtualSize), int(section.SizeOfRawData)),
        )
        for section in pe.sections
        if section.Characteristics & 0x20000000
    ]
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    offsets = set(args.field_offset)
    method_rows: list[dict[str, object]] = []

    for candidate in candidates:
        start = int(candidate["address"])
        if not any(low <= start < high for low, high in executable_ranges):
            continue
        next_index = bisect.bisect_right(addresses, start)
        if next_index >= len(addresses):
            continue
        natural_end = addresses[next_index]
        end = min(natural_end, start + args.maximum_method_bytes)
        if end <= start:
            continue
        data = pe.get_data(start, end - start)
        instructions = list(md.disasm(data, start))
        hit_indexes: list[int] = []
        accesses: list[dict[str, object]] = []
        for index, instruction in enumerate(instructions):
            for operand_index, operand in enumerate(instruction.operands):
                if operand.type != X86_OP_MEM or int(operand.mem.disp) not in offsets:
                    continue
                base = instruction.reg_name(operand.mem.base) if operand.mem.base else ""
                if base in EXCLUDED_BASES or not base:
                    continue
                index_register = (
                    instruction.reg_name(operand.mem.index) if operand.mem.index else ""
                )
                hit_indexes.append(index)
                accesses.append(
                    {
                        **instruction_row(instruction),
                        "operand_index": operand_index,
                        "field_offset": int(operand.mem.disp),
                        "base_register": base,
                        "index_register": index_register,
                        "scale": int(operand.mem.scale),
                    }
                )
        if not accesses:
            continue
        context_indexes: set[int] = set()
        for index in hit_indexes:
            low = max(0, index - args.context_instructions)
            high = min(len(instructions), index + args.context_instructions + 1)
            context_indexes.update(range(low, high))
        method_rows.append(
            {
                "name": candidate["name"],
                "signature": candidate["signature"],
                "start_rva": start,
                "natural_end_rva": natural_end,
                "scanned_end_rva": end,
                "method_bytes_scanned": len(data),
                "method_region_sha256": hashlib.sha256(data).hexdigest(),
                "field_accesses": accesses,
                "context_disassembly": [
                    instruction_row(instructions[index]) for index in sorted(context_indexes)
                ],
            }
        )

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-field-offset-consumer-audit.py",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "bytes": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
        },
        "script_json": {
            "path": str(args.script_json.resolve()),
            "bytes": args.script_json.stat().st_size,
            "sha256": sha256(args.script_json),
        },
        "selection": {
            "signature_fragments": args.signature_fragment,
            "field_offsets": sorted(offsets),
            "maximum_method_bytes": args.maximum_method_bytes,
            "context_instructions": args.context_instructions,
        },
        "candidate_methods": method_rows,
        "summary": {
            "method_index_entries": len(methods),
            "signature_matched_methods": len(candidates),
            "methods_with_selected_offset_accesses": len(method_rows),
            "selected_offset_accesses": sum(
                len(row["field_accesses"]) for row in method_rows
            ),
        },
        "policy": {
            "script_method_label_is_formula_authority": False,
            "matching_displacement_is_object_identity": False,
            "stack_and_instruction_pointer_relative_operands_excluded": True,
            "register_dataflow_review_required": True,
            "provider_rdps_credit_allowed": False,
        },
    }
    encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        if args.output.exists():
            raise SystemExit(f"refusing to overwrite {args.output}")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(json.dumps(report["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
