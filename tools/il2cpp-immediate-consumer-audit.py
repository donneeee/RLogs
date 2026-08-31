#!/usr/bin/env python3
"""Find bounded native methods containing selected exact integer immediates.

The audit first locates the little-endian bytes for each requested integer in
executable PE sections. It then disassembles only the enclosing IL2CPP method
interval inferred from Il2CppDumper's dump.cs and accepts a hit only when
Capstone decodes the requested value as an immediate operand. Raw byte matches,
method interval names, and immediate matches remain evidence rather than
automatic object identity or formula authority.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import mmap
import re
import struct
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


def parse_target(raw: str) -> int:
    value = int(raw, 0)
    if value < 0 or value > 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("targets must fit an unsigned 32-bit integer")
    return value


def read_dump_method_index(path: Path) -> list[dict[str, object]]:
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
            if "(" not in declaration or ")" not in declaration:
                pending_rva = None
                continue
            qualified_type = f"{namespace}.{type_name}" if namespace else type_name
            methods.append(
                {
                    "address": pending_rva,
                    "name": f"{qualified_type}.{declaration.rstrip(' {')}",
                }
            )
            pending_rva = None
    methods.sort(key=lambda item: (int(item["address"]), str(item["name"])))
    return methods


def instruction_row(instruction: object, targets: set[int]) -> dict[str, object]:
    immediate_hits = sorted(
        {
            int(operand.imm)
            for operand in instruction.operands
            if operand.type == X86_OP_IMM and int(operand.imm) in targets
        }
    )
    return {
        "rva": int(instruction.address),
        "size": int(instruction.size),
        "mnemonic": str(instruction.mnemonic),
        "operands": str(instruction.op_str),
        "immediate_hits": immediate_hits,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--maximum-method-bytes", type=int, default=1_048_576)
    parser.add_argument("--context-instructions", type=int, default=6)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.maximum_method_bytes <= 0 or args.context_instructions < 0:
        parser.error("method and context bounds must be non-negative")

    targets = set(args.target)
    methods = read_dump_method_index(args.dump)
    by_address: dict[int, list[str]] = {}
    for method in methods:
        address = int(method["address"])
        if address > 0:
            by_address.setdefault(address, []).append(str(method["name"]))
    addresses = sorted(by_address)

    pe = pefile.PE(str(args.binary), fast_load=True)
    sections = []
    for section in pe.sections:
        if not (section.Characteristics & 0x20000000):
            continue
        sections.append(
            {
                "name": section.Name.rstrip(b"\0").decode("ascii", errors="replace"),
                "file_start": int(section.PointerToRawData),
                "file_end": int(section.PointerToRawData) + int(section.SizeOfRawData),
                "rva_start": int(section.VirtualAddress),
                "rva_end": int(section.VirtualAddress)
                + max(int(section.Misc_VirtualSize), int(section.SizeOfRawData)),
            }
        )

    raw_hits: list[dict[str, object]] = []
    with args.binary.open("rb") as source:
        mapped = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            for target in sorted(targets):
                needle = struct.pack("<I", target)
                cursor = 0
                while True:
                    file_offset = mapped.find(needle, cursor)
                    if file_offset < 0:
                        break
                    section = next(
                        (
                            candidate
                            for candidate in sections
                            if int(candidate["file_start"]) <= file_offset
                            < int(candidate["file_end"])
                        ),
                        None,
                    )
                    if section is not None:
                        rva = int(section["rva_start"]) + file_offset - int(section["file_start"])
                        raw_hits.append(
                            {
                                "target": target,
                                "file_offset": file_offset,
                                "rva": rva,
                                "section": str(section["name"]),
                            }
                        )
                    cursor = file_offset + 1
        finally:
            mapped.close()

    candidate_starts: set[int] = set()
    unmatched_raw_hits: list[dict[str, object]] = []
    for hit in raw_hits:
        rva = int(hit["rva"])
        index = bisect.bisect_right(addresses, rva) - 1
        if index < 0 or index + 1 >= len(addresses):
            unmatched_raw_hits.append({**hit, "reason": "no-bounded-method-interval"})
            continue
        start = addresses[index]
        end = addresses[index + 1]
        section = next(
            (candidate for candidate in sections if int(candidate["rva_start"]) <= start < int(candidate["rva_end"])),
            None,
        )
        if (
            section is None
            or not (start <= rva < end)
            or end - start <= 0
            or end - start > args.maximum_method_bytes
            or end > int(section["rva_end"])
        ):
            unmatched_raw_hits.append({**hit, "reason": "method-interval-outside-bounds"})
            continue
        hit["enclosing_method_start_rva"] = start
        hit["enclosing_method_end_rva"] = end
        hit["enclosing_method_offset"] = rva - start
        hit["enclosing_method_names"] = sorted(by_address[start])
        candidate_starts.add(start)

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    method_rows: list[dict[str, object]] = []
    decoded_hit_spans: list[tuple[int, int, int]] = []
    for start in sorted(candidate_starts):
        index = bisect.bisect_left(addresses, start)
        end = addresses[index + 1]
        data = pe.get_data(start, end - start)
        instructions = list(md.disasm(data, start))
        hit_indexes = []
        hits = []
        for instruction_index, instruction in enumerate(instructions):
            row = instruction_row(instruction, targets)
            if not row["immediate_hits"]:
                continue
            hit_indexes.append(instruction_index)
            hits.append(row)
            for target in row["immediate_hits"]:
                decoded_hit_spans.append(
                    (int(row["rva"]), int(row["rva"]) + int(row["size"]), int(target))
                )
        if not hits:
            continue
        context_indexes: set[int] = set()
        for instruction_index in hit_indexes:
            low = max(0, instruction_index - args.context_instructions)
            high = min(len(instructions), instruction_index + args.context_instructions + 1)
            context_indexes.update(range(low, high))
        method_rows.append(
            {
                "start_rva": start,
                "end_rva": end,
                "bytes_scanned": len(data),
                "method_region_sha256": hashlib.sha256(data).hexdigest(),
                "names": sorted(by_address[start]),
                "immediate_hits": hits,
                "context_disassembly": [
                    instruction_row(instructions[index], targets)
                    for index in sorted(context_indexes)
                ],
            }
        )

    for hit in raw_hits:
        if not any(
            start <= int(hit["rva"]) < end and target == int(hit["target"])
            for start, end, target in decoded_hit_spans
        ):
            if not any(
                int(existing["rva"]) == int(hit["rva"])
                and int(existing["target"]) == int(hit["target"])
                for existing in unmatched_raw_hits
            ):
                unmatched_raw_hits.append({**hit, "reason": "not-a-decoded-immediate-operand"})

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-immediate-consumer-audit.py",
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
            "unique_method_rvas": len(addresses),
        },
        "selection": {
            "targets": sorted(targets),
            "maximum_method_bytes": args.maximum_method_bytes,
            "context_instructions": args.context_instructions,
        },
        "executable_sections": [
            {
                "name": section["name"],
                "rva_start": section["rva_start"],
                "rva_end": section["rva_end"],
            }
            for section in sections
        ],
        "candidate_methods": method_rows,
        "unmatched_raw_hits": sorted(
            unmatched_raw_hits,
            key=lambda row: (int(row["target"]), int(row["rva"]), str(row["reason"])),
        ),
        "summary": {
            "targets": len(targets),
            "raw_executable_section_hits": len(raw_hits),
            "candidate_method_intervals": len(candidate_starts),
            "methods_with_decoded_immediate_hits": len(method_rows),
            "decoded_immediate_instructions": sum(len(row["immediate_hits"]) for row in method_rows),
            "decoded_target_occurrences": sum(
                len(hit["immediate_hits"])
                for row in method_rows
                for hit in row["immediate_hits"]
            ),
            "unmatched_raw_hits": len(unmatched_raw_hits),
        },
        "policy": {
            "raw_byte_match_is_instruction_evidence": False,
            "decoded_immediate_is_attribute_identity": False,
            "containing_method_is_address_interval_inference": True,
            "register_dataflow_and_callgraph_review_required": True,
            "formula_authority": False,
            "runtime_authority": False,
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
