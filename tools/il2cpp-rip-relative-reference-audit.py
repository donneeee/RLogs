#!/usr/bin/env python3
"""Audit exact RIP-relative references to selected RVAs in a PE image.

This is an evidence extractor, not a semantic decompiler. It decodes each PE
executable section one at a time, retains instructions whose RIP-relative
effective address equals an explicitly selected RVA, and reports the largest
section buffer plus measured process peak working set. Runtime-computed and
indirect references remain outside its claims.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import re
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_64, Cs


RIP_OPERAND_RE = re.compile(r"\[rip\s*([+-])\s*0x([0-9a-f]+)\]", re.IGNORECASE)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_file_range(path: Path, offset: int, length: int) -> str:
    digest = hashlib.sha256()
    remaining = length
    with path.open("rb") as source:
        source.seek(offset)
        while remaining:
            chunk = source.read(min(1024 * 1024, remaining))
            if not chunk:
                raise SystemExit(f"short read hashing {path} at file offset {offset}")
            digest.update(chunk)
            remaining -= len(chunk)
    return digest.hexdigest()


def parse_target(raw: str) -> tuple[int, str]:
    value, separator, label = raw.partition("=")
    try:
        rva = int(value, 0)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"invalid target RVA {value!r}") from error
    if rva < 0:
        raise argparse.ArgumentTypeError("target RVAs must be non-negative")
    return rva, label if separator and label else f"rva-0x{rva:x}"


def peak_working_set_bytes() -> int | None:
    if not hasattr(ctypes, "windll"):
        return None

    class ProcessMemoryCounters(ctypes.Structure):
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

    counters = ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    process = ctypes.windll.kernel32.GetCurrentProcess()
    get_process_memory_info = ctypes.windll.psapi.GetProcessMemoryInfo
    get_process_memory_info.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
    get_process_memory_info.restype = ctypes.c_int
    ok = get_process_memory_info(process, ctypes.byref(counters), counters.cb)
    return int(counters.peak_working_set_size) if ok else None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--target", action="append", required=True, type=parse_target)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--chunk-bytes", type=int, default=1024 * 1024)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"refusing to overwrite {args.output}")
    if args.chunk_bytes < 4096:
        raise SystemExit("--chunk-bytes must be at least 4096")
    targets: dict[int, str] = {}
    for rva, label in args.target:
        previous = targets.get(rva)
        if previous is not None and previous != label:
            raise SystemExit(f"target RVA 0x{rva:x} has conflicting labels")
        targets[rva] = label

    pe = pefile.PE(str(args.binary), fast_load=True)
    decoder = Cs(CS_ARCH_X86, CS_MODE_64)
    decoder.skipdata = True
    references: list[dict[str, object]] = []
    sections: list[dict[str, object]] = []
    maximum_section_bytes = 0
    maximum_decoder_buffer_bytes = 0
    decoded_instructions = 0
    skipped_data_rows = 0

    for section in pe.sections:
        if not (section.Characteristics & 0x20000000):
            continue
        section_rva = int(section.VirtualAddress)
        section_bytes = int(section.SizeOfRawData)
        maximum_section_bytes = max(maximum_section_bytes, section_bytes)
        section_instruction_count = 0
        section_skipped_rows = 0
        cursor = 0
        while cursor < section_bytes:
            nominal_end = min(section_bytes, cursor + args.chunk_bytes)
            # x86-64 instructions are at most 15 bytes. The overlap lets the
            # final instruction beginning in this nominal chunk decode fully.
            read_end = min(section_bytes, nominal_end + 15)
            data = pe.get_data(section_rva + cursor, read_end - cursor)
            if not data:
                raise SystemExit(
                    f"short PE read at RVA 0x{section_rva + cursor:x}"
                )
            maximum_decoder_buffer_bytes = max(maximum_decoder_buffer_bytes, len(data))
            next_cursor = cursor
            for address, size, mnemonic, operands in decoder.disasm_lite(
                data, section_rva + cursor
            ):
                instruction_offset = address - section_rva
                if instruction_offset >= nominal_end:
                    break
                next_cursor = max(next_cursor, instruction_offset + size)
                section_instruction_count += 1
                if mnemonic == ".byte":
                    section_skipped_rows += 1
                    continue
                for sign, displacement_hex in RIP_OPERAND_RE.findall(operands):
                    displacement = int(displacement_hex, 16)
                    if sign == "-":
                        displacement = -displacement
                    effective_rva = address + size + displacement
                    label = targets.get(effective_rva)
                    if label is None:
                        continue
                    buffer_offset = instruction_offset - cursor
                    references.append(
                        {
                            "instruction_rva": address,
                            "instruction_bytes_hex": data[
                                buffer_offset : buffer_offset + size
                            ].hex(),
                            "mnemonic": mnemonic,
                            "operands": operands,
                            "effective_target_rva": effective_rva,
                            "target_label": label,
                            "section": section.Name.rstrip(b"\0").decode(
                                "ascii", errors="replace"
                            ),
                        }
                    )
            if next_cursor <= cursor:
                raise SystemExit(
                    f"decoder made no progress at RVA 0x{section_rva + cursor:x}"
                )
            cursor = next_cursor
        decoded_instructions += section_instruction_count
        skipped_data_rows += section_skipped_rows
        sections.append(
            {
                "name": section.Name.rstrip(b"\0").decode("ascii", errors="replace"),
                "rva": section_rva,
                "bytes": section_bytes,
                "sha256": sha256_file_range(
                    args.binary, int(section.PointerToRawData), section_bytes
                ),
                "decoded_instruction_rows": section_instruction_count,
                "skipped_data_rows": section_skipped_rows,
            }
        )

    references.sort(
        key=lambda row: (int(row["instruction_rva"]), int(row["effective_target_rva"]))
    )
    peak_bytes = peak_working_set_bytes()
    report = {
        "schema_version": 1,
        "generated_by": "tools/il2cpp-rip-relative-reference-audit.py",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "bytes": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
        },
        "targets": [
            {"rva": rva, "label": label} for rva, label in sorted(targets.items())
        ],
        "executable_sections": sections,
        "references": references,
        "resource_bounds": {
            "one_decoder_chunk_buffered_at_a_time": True,
            "configured_chunk_bytes": args.chunk_bytes,
            "x86_instruction_overlap_bytes": 15,
            "maximum_decoder_buffer_bytes": maximum_decoder_buffer_bytes,
            "maximum_section_bytes": maximum_section_bytes,
            "measured_process_peak_working_set_bytes": peak_bytes,
        },
        "summary": {
            "target_rvas": len(targets),
            "executable_sections": len(sections),
            "decoded_instruction_rows": decoded_instructions,
            "skipped_data_rows": skipped_data_rows,
            "exact_rip_relative_references": len(references),
            "target_rvas_with_references": len(
                {int(row["effective_target_rva"]) for row in references}
            ),
        },
        "policy": {
            "exact_effective_rva_match": True,
            "target_labels_are_semantic_authority": False,
            "runtime_computed_and_indirect_references_enumerated": False,
            "absence_of_direct_rip_reference_proves_no_semantic_access": False,
            "register_dataflow_review_required": True,
            "provider_rdps_credit_allowed": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"], indent=2))
    print(json.dumps(report["resource_bounds"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
