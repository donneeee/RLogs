#!/usr/bin/env python3
"""Audit bounded direct calls to exact IL2CPP generic/native method RVAs.

Unlike the older direct-callsite extractor, this index includes Il2CppDumper's
``GenericInstMethod`` RVA/name rows.  Raw ``E8`` byte candidates are accepted as
direct calls only when disassembly from a bounded enclosing method start reaches
the candidate on an instruction boundary and resolves it to the requested RVA.

The report is evidence, not a decompiler or formula authority.  In particular,
the optional RDX last-writer classification is only an x64 ABI review aid.
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
from capstone.x86_const import X86_OP_IMM, X86_OP_MEM, X86_OP_REG


DUMP_RVA_RE = re.compile(r"^\s*// RVA: 0x([0-9A-Fa-f]+)\b")
DUMP_GENERIC_RVA_RE = re.compile(r"^\s*\|-RVA: 0x([0-9A-Fa-f]+)\b")
DUMP_GENERIC_NAME_RE = re.compile(r"^\s*\|-(?!RVA:)(.+?)\s*$")
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
    if "=" in raw:
        label, value = raw.rsplit("=", 1)
    else:
        label, value = raw, raw
    rva = int(value, 0)
    if rva <= 0 or rva > 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("target RVAs must fit a nonzero uint32")
    return rva, label.strip()


def qualify_generic_name(namespace: str, raw_name: str) -> str:
    if namespace and not raw_name.startswith(f"{namespace}."):
        return f"{namespace}.{raw_name}"
    return raw_name


def read_dump_method_index(path: Path) -> tuple[list[dict[str, object]], dict[str, int]]:
    methods: list[dict[str, object]] = []
    namespace = ""
    type_name = "<unknown-type>"
    pending_rva: int | None = None
    generic_rva: int | None = None
    ordinary_count = 0
    generic_count = 0
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
            generic_rva_match = DUMP_GENERIC_RVA_RE.match(line)
            if generic_rva_match:
                generic_rva = int(generic_rva_match.group(1), 16)
                continue
            generic_name_match = DUMP_GENERIC_NAME_RE.match(line)
            if generic_name_match and generic_rva is not None:
                methods.append(
                    {
                        "address": generic_rva,
                        "name": qualify_generic_name(
                            namespace, generic_name_match.group(1).strip()
                        ),
                        "kind": "generic-instantiation",
                    }
                )
                generic_count += 1
                continue
            if generic_rva is not None and line.strip() == "*/":
                generic_rva = None
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
                    "kind": "ordinary",
                }
            )
            ordinary_count += 1
            pending_rva = None
    methods.sort(
        key=lambda item: (int(item["address"]), str(item["kind"]), str(item["name"]))
    )
    return methods, {
        "ordinary_method_entries": ordinary_count,
        "generic_instantiation_entries": generic_count,
    }


def rdx_last_writer(instructions: list[object], call_index: int) -> dict[str, object]:
    for instruction in reversed(instructions[max(0, call_index - 24) : call_index]):
        try:
            _, written = instruction.regs_access()
        except Exception:
            continue
        written_names = {instruction.reg_name(register) for register in written}
        if not ({"rdx", "edx", "dx", "dl", "dh"} & written_names):
            continue
        classification = "computed-or-dynamic"
        if instruction.mnemonic == "mov" and len(instruction.operands) >= 2:
            source = instruction.operands[1]
            if source.type == X86_OP_IMM:
                classification = "immediate"
            elif source.type == X86_OP_REG:
                classification = "register-derived"
            elif source.type == X86_OP_MEM:
                classification = "memory-derived"
        elif instruction.mnemonic == "xor" and instruction.op_str in {
            "edx, edx",
            "rdx, rdx",
        }:
            classification = "immediate-zero"
        elif instruction.mnemonic == "lea":
            classification = "address-or-arithmetic-derived"
        return {
            "rva": int(instruction.address),
            "mnemonic": str(instruction.mnemonic),
            "operands": str(instruction.op_str),
            "classification": classification,
        }
    return {"classification": "not-found-within-24-instructions"}


def instruction_row(instruction: object, is_target_call: bool) -> dict[str, object]:
    return {
        "rva": int(instruction.address),
        "size": int(instruction.size),
        "mnemonic": str(instruction.mnemonic),
        "operands": str(instruction.op_str),
        "is_target_call": is_target_call,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--maximum-method-bytes", type=int, default=131_072)
    parser.add_argument("--context-instructions", type=int, default=10)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.maximum_method_bytes <= 0 or args.context_instructions < 0:
        parser.error("method and context bounds must be non-negative")
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite {args.output}")

    target_labels: dict[int, list[str]] = {}
    for rva, label in args.target:
        target_labels.setdefault(rva, []).append(label)

    methods, method_counts = read_dump_method_index(args.dump)
    by_address: dict[int, list[dict[str, str]]] = {}
    for method in methods:
        address = int(method["address"])
        if address > 0:
            by_address.setdefault(address, []).append(
                {"name": str(method["name"]), "kind": str(method["kind"])}
            )
    addresses = sorted(by_address)

    pe = pefile.PE(str(args.binary), fast_load=True)
    sections: list[dict[str, object]] = []
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

    raw_candidates: list[dict[str, object]] = []
    with args.binary.open("rb") as source:
        mapped = mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ)
        try:
            for section in sections:
                start = int(section["file_start"])
                end = min(int(section["file_end"]), len(mapped))
                cursor = start
                while True:
                    file_offset = mapped.find(b"\xE8", cursor, end)
                    if file_offset < 0 or file_offset + 5 > end:
                        break
                    displacement = struct.unpack_from("<i", mapped, file_offset + 1)[0]
                    call_rva = (
                        int(section["rva_start"]) + file_offset - int(section["file_start"])
                    )
                    destination = call_rva + 5 + displacement
                    if destination in target_labels:
                        raw_candidates.append(
                            {
                                "call_rva": call_rva,
                                "target_rva": destination,
                                "target_labels": sorted(target_labels[destination]),
                                "section": str(section["name"]),
                            }
                        )
                    cursor = file_offset + 1
        finally:
            mapped.close()

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    confirmed: list[dict[str, object]] = []
    rejected: list[dict[str, object]] = []
    disassembly_cache: dict[tuple[int, int], list[object]] = {}
    for candidate in raw_candidates:
        call_rva = int(candidate["call_rva"])
        index = bisect.bisect_right(addresses, call_rva) - 1
        if index < 0 or index + 1 >= len(addresses):
            rejected.append({**candidate, "reason": "no-bounded-method-interval"})
            continue
        method_start = addresses[index]
        method_end = addresses[index + 1]
        section = next(
            (
                row
                for row in sections
                if int(row["rva_start"]) <= method_start < int(row["rva_end"])
            ),
            None,
        )
        if (
            section is None
            or not method_start <= call_rva < method_end
            or method_end - method_start <= 0
            or method_end - method_start > args.maximum_method_bytes
            or method_end > int(section["rva_end"])
        ):
            rejected.append({**candidate, "reason": "method-interval-outside-bounds"})
            continue
        key = (method_start, method_end)
        instructions = disassembly_cache.get(key)
        if instructions is None:
            instructions = list(md.disasm(pe.get_data(method_start, method_end - method_start), method_start))
            disassembly_cache[key] = instructions
        call_index = next(
            (
                offset
                for offset, instruction in enumerate(instructions)
                if int(instruction.address) == call_rva
                and instruction.mnemonic == "call"
                and instruction.operands
                and instruction.operands[0].type == X86_OP_IMM
                and int(instruction.operands[0].imm) == int(candidate["target_rva"])
            ),
            None,
        )
        if call_index is None:
            rejected.append({**candidate, "reason": "not-decoded-at-method-instruction-boundary"})
            continue
        low = max(0, call_index - args.context_instructions)
        high = min(len(instructions), call_index + args.context_instructions + 1)
        confirmed.append(
            {
                **candidate,
                "caller_start_rva": method_start,
                "caller_end_rva": method_end,
                "caller_offset": call_rva - method_start,
                "caller_names": sorted(
                    by_address[method_start], key=lambda row: (row["kind"], row["name"])
                ),
                "rdx_last_writer_review_aid": rdx_last_writer(instructions, call_index),
                "context_disassembly": [
                    instruction_row(instructions[offset], offset == call_index)
                    for offset in range(low, high)
                ],
            }
        )

    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-generic-callsite-audit.py",
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
            **method_counts,
            "total_method_entries": len(methods),
            "unique_method_rvas": len(addresses),
        },
        "selection": {
            "targets": [
                {"rva": rva, "labels": sorted(labels)}
                for rva, labels in sorted(target_labels.items())
            ],
            "maximum_method_bytes": args.maximum_method_bytes,
            "context_instructions": args.context_instructions,
        },
        "executable_sections": [
            {
                "name": row["name"],
                "rva_start": row["rva_start"],
                "rva_end": row["rva_end"],
            }
            for row in sections
        ],
        "confirmed_direct_callsites": sorted(
            confirmed, key=lambda row: (int(row["target_rva"]), int(row["call_rva"]))
        ),
        "rejected_raw_e8_candidates": sorted(
            rejected,
            key=lambda row: (int(row["target_rva"]), int(row["call_rva"]), str(row["reason"])),
        ),
        "summary": {
            "targets": len(target_labels),
            "raw_e8_candidates": len(raw_candidates),
            "confirmed_direct_callsites": len(confirmed),
            "unique_confirmed_caller_rvas": len(
                {int(row["caller_start_rva"]) for row in confirmed}
            ),
            "rejected_raw_e8_candidates": len(rejected),
            "confirmed_callsites_with_immediate_rdx_writer": sum(
                row["rdx_last_writer_review_aid"]["classification"]
                in {"immediate", "immediate-zero"}
                for row in confirmed
            ),
        },
        "policy": {
            "generic_instantiation_rvas_are_indexed": True,
            "raw_e8_match_is_direct_call_evidence": False,
            "confirmed_call_requires_bounded_method_disassembly": True,
            "containing_method_is_address_interval_inference": True,
            "rdx_last_writer_is_abi_review_aid_only": True,
            "indirect_calls_are_not_claimed_absent": True,
            "computed_or_table_driven_consumers_are_not_claimed_absent": True,
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
