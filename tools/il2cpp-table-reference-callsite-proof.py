#!/usr/bin/env python3
"""Prove decoded-table relationships from current-build IL2CPP callsites.

This tool deliberately does not promote a relationship from matching numbers,
similar names, or a shared consumer alone. It starts with the retained namespace
candidates emitted by DecodedTableReferenceGraph.gen, then proves that a real
current-client method passes the source row's field value as the key to a
table-key lookup on the candidate target table. Shared direct-call consumers
are retained as corroboration even when that data-flow proof is unavailable.

The dump parser uses Il2CppDumper's RVA comments and method declarations.  The
PE scanner accepts only x64 relative CALL instructions (E8 rel32).  Caller names
are interval-attributed to the closest preceding dumped method; this is stated
in the output and never treated as instruction-level formula proof.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
import struct
from collections import defaultdict
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_64, Cs
from capstone.x86_const import X86_OP_IMM, X86_OP_MEM, X86_OP_REG


CLASS_RE = re.compile(r"^public class\s+([A-Za-z0-9_`]+)")
NAMESPACE_RE = re.compile(r"^// Namespace:\s*(.*)$")
RVA_RE = re.compile(r"^\s*// RVA: 0x([0-9A-Fa-f]+)")
METHOD_RE = re.compile(
    r"^\s*(?:public|private|protected|internal)(?:\s+static)?(?:\s+virtual)?"
    r"(?:\s+sealed)?\s+.+?\s+([A-Za-z0-9_`.<>]+)\((.*)\)\s*\{\s*\}\s*$"
)
GENERIC_INST_RVA_RE = re.compile(r"^\s*\|-RVA: 0x([0-9A-Fa-f]+)")
GENERIC_INST_NAME_RE = re.compile(r"^\s*\|-([^|].*)$")


VOLATILE_REGISTERS = {"rax", "rcx", "rdx", "r8", "r9", "r10", "r11"}
ARGUMENT_REGISTERS = ("rcx", "rdx", "r8", "r9")
MOVE_MNEMONICS = {"mov", "movabs", "movsx", "movsxd", "movzx", "lea"}
ARITHMETIC_MNEMONICS = {
    "add", "and", "dec", "imul", "inc", "neg", "not", "or", "rol", "ror",
    "sal", "sar", "shl", "shr", "sub", "xor",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_jsonl(path: Path) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    with path.open("r", encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise ValueError(f"invalid JSONL at {path}:{line_number}: {error}") from error
    return rows


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def parse_dump(path: Path) -> list[dict[str, object]]:
    methods: list[dict[str, object]] = []
    namespace = ""
    class_name: str | None = None
    pending_rva: int | None = None
    with path.open("r", encoding="utf-8", errors="replace") as source:
        for raw_line in source:
            line = raw_line.rstrip("\r\n")
            namespace_match = NAMESPACE_RE.match(line)
            if namespace_match:
                namespace = namespace_match.group(1).strip()
                class_name = None
                continue
            class_match = CLASS_RE.match(line)
            if class_match:
                class_name = class_match.group(1)
                continue
            rva_match = RVA_RE.match(line)
            if rva_match:
                pending_rva = int(rva_match.group(1), 16)
                continue
            if pending_rva is None or class_name is None:
                continue
            method_match = METHOD_RE.match(line)
            if not method_match:
                continue
            short_name = method_match.group(1)
            parameters = method_match.group(2).strip()
            qualified_class = f"{namespace}.{class_name}" if namespace else class_name
            methods.append(
                {
                    "rva": pending_rva,
                    "namespace": namespace,
                    "class": class_name,
                    "qualified_class": qualified_class,
                    "method": short_name,
                    "parameters": parameters,
                    "display_name": f"{qualified_class}.{short_name}({parameters})",
                }
            )
            pending_rva = None
    methods.sort(key=lambda item: (int(item["rva"]), str(item["display_name"])))
    return methods


def parse_generic_inst_rvas(path: Path) -> dict[str, list[int]]:
    """Read Il2CppDumper's GenericInstMethod entries that have no normal RVA."""
    results: dict[str, list[int]] = defaultdict(list)
    pending_rva: int | None = None
    with path.open("r", encoding="utf-8", errors="replace") as source:
        for raw_line in source:
            line = raw_line.rstrip("\r\n")
            rva_match = GENERIC_INST_RVA_RE.match(line)
            if rva_match:
                pending_rva = int(rva_match.group(1), 16)
                continue
            if pending_rva is None:
                continue
            name_match = GENERIC_INST_NAME_RE.match(line)
            if not name_match:
                continue
            name = name_match.group(1).strip()
            results[name].append(pending_rva)
            pending_rva = None
    return {key: sorted(set(values)) for key, values in results.items()}


def canonical_register(name: str) -> str:
    aliases = {
        "al": "rax", "ah": "rax", "ax": "rax", "eax": "rax", "rax": "rax",
        "bl": "rbx", "bh": "rbx", "bx": "rbx", "ebx": "rbx", "rbx": "rbx",
        "cl": "rcx", "ch": "rcx", "cx": "rcx", "ecx": "rcx", "rcx": "rcx",
        "dl": "rdx", "dh": "rdx", "dx": "rdx", "edx": "rdx", "rdx": "rdx",
        "sil": "rsi", "si": "rsi", "esi": "rsi", "rsi": "rsi",
        "dil": "rdi", "di": "rdi", "edi": "rdi", "rdi": "rdi",
        "bpl": "rbp", "bp": "rbp", "ebp": "rbp", "rbp": "rbp",
        "spl": "rsp", "sp": "rsp", "esp": "rsp", "rsp": "rsp",
    }
    lowered = name.lower()
    if lowered in aliases:
        return aliases[lowered]
    match = re.fullmatch(r"r(8|9|1[0-5])(?:b|w|d)?", lowered)
    return f"r{match.group(1)}" if match else lowered


def instruction_text(instruction: object) -> str:
    return f"0x{int(instruction.address):X}: {instruction.mnemonic} {instruction.op_str}".rstrip()


def operand_register(instruction: object, operand: object) -> str | None:
    if operand.type != X86_OP_REG:
        return None
    return canonical_register(instruction.reg_name(operand.reg))


def stack_slot(instruction: object, operand: object) -> tuple[str, int] | None:
    if operand.type != X86_OP_MEM:
        return None
    base = canonical_register(instruction.reg_name(operand.mem.base)) if operand.mem.base else ""
    index = canonical_register(instruction.reg_name(operand.mem.index)) if operand.mem.index else ""
    if base not in {"rsp", "rbp"} or index:
        return None
    return base, int(operand.mem.disp)


def operand_taint(
    instruction: object,
    operand: object,
    registers: dict[str, set[str]],
    stack: dict[tuple[str, int], set[str]],
) -> set[str]:
    register = operand_register(instruction, operand)
    if register is not None:
        return set(registers.get(register, set()))
    if operand.type != X86_OP_MEM:
        return set()
    slot = stack_slot(instruction, operand)
    if slot is not None:
        return set(stack.get(slot, set()))
    result: set[str] = set()
    if operand.mem.base:
        result.update(registers.get(canonical_register(instruction.reg_name(operand.mem.base)), set()))
    if operand.mem.index:
        result.update(registers.get(canonical_register(instruction.reg_name(operand.mem.index)), set()))
    return result


def set_operand_taint(
    instruction: object,
    operand: object,
    taint: set[str],
    registers: dict[str, set[str]],
    stack: dict[tuple[str, int], set[str]],
) -> None:
    register = operand_register(instruction, operand)
    if register is not None:
        registers[register] = set(taint)
        return
    slot = stack_slot(instruction, operand)
    if slot is not None:
        stack[slot] = set(taint)


def prove_target_lookup_dataflow(
    instructions: list[object],
    source_call_rvas: set[int],
    target_call_rvas: set[int],
    lookup_methods_by_rva: dict[int, list[str]],
) -> list[dict[str, object]]:
    """Conservatively prove source-derived key -> target-table lookup.

    Taint S is introduced by the selected source getter and T by the selected
    target GetTable accessor. Straight-line register, stack, memory-base, and
    source-only helper-call propagation is retained. A proof is emitted only
    when an exact current-build ZTable<int, object> key-lookup method receives
    RCX=T and RDX=S.
    """
    registers: dict[str, set[str]] = defaultdict(set)
    stack: dict[tuple[str, int], set[str]] = defaultdict(set)
    origins: dict[str, list[int]] = {"S": [], "T": []}
    trace: list[str] = []
    proofs: list[dict[str, object]] = []
    for instruction in instructions:
        mnemonic = instruction.mnemonic.lower()
        address = int(instruction.address)
        if mnemonic == ".byte":
            registers.clear()
            stack.clear()
            trace.clear()
            continue
        operands = list(instruction.operands)
        if mnemonic == "call" and operands and operands[0].type == X86_OP_IMM:
            destination = int(operands[0].imm)
            argument_taint = {
                register: sorted(registers.get(register, set()))
                for register in ARGUMENT_REGISTERS
                if registers.get(register)
            }
            if destination in lookup_methods_by_rva:
                if "T" in registers.get("rcx", set()) and "S" in registers.get("rdx", set()):
                    proofs.append(
                        {
                            "lookup_call_rva": address,
                            "lookup_method_rva": destination,
                            "lookup_methods": lookup_methods_by_rva[destination],
                            "source_origin_calls": sorted(set(origins["S"])),
                            "target_origin_calls": sorted(set(origins["T"])),
                            "argument_taint": argument_taint,
                            "instruction": instruction_text(instruction),
                            "trace_tail": trace[-24:],
                        }
                    )
            return_taint: set[str] = set()
            if address in source_call_rvas:
                return_taint = {"S"}
                origins["S"].append(address)
            elif address in target_call_rvas:
                return_taint = {"T"}
                origins["T"].append(address)
            else:
                flattened = {value for values in argument_taint.values() for value in values}
                # A helper receiving only a source-derived value may return a
                # transformed/indexed source key. Never propagate target-table
                # identity through an unknown helper.
                if flattened == {"S"}:
                    return_taint = {"S"}
            for register in VOLATILE_REGISTERS:
                registers[register] = set()
            registers["rax"] = return_taint
            if return_taint or argument_taint:
                trace.append(instruction_text(instruction))
            continue

        if mnemonic in MOVE_MNEMONICS and len(operands) >= 2:
            taint = operand_taint(instruction, operands[1], registers, stack)
            set_operand_taint(instruction, operands[0], taint, registers, stack)
            if taint:
                trace.append(instruction_text(instruction))
            continue

        if mnemonic in ARITHMETIC_MNEMONICS and operands:
            if mnemonic == "xor" and len(operands) >= 2:
                left = operand_register(instruction, operands[0])
                right = operand_register(instruction, operands[1])
                if left is not None and left == right:
                    set_operand_taint(instruction, operands[0], set(), registers, stack)
                    continue
            taint: set[str] = set()
            for operand in operands:
                taint.update(operand_taint(instruction, operand, registers, stack))
            set_operand_taint(instruction, operands[0], taint, registers, stack)
            if taint:
                trace.append(instruction_text(instruction))
            continue

        if mnemonic in {"ret", "int3"}:
            registers.clear()
            stack.clear()
            trace.clear()
    return proofs


def target_methods(
    methods: list[dict[str, object]],
    candidate_rows: list[dict[str, object]],
) -> tuple[dict[str, list[dict[str, object]]], dict[str, list[dict[str, object]]]]:
    getters: dict[str, list[dict[str, object]]] = defaultdict(list)
    table_accessors: dict[str, list[dict[str, object]]] = defaultdict(list)
    wanted_getters = {
        f"{row['source_table']}Base|get_{row['field']}"
        for row in candidate_rows
    }
    wanted_tables = {
        f"{candidate['target_table']}Base"
        for row in candidate_rows
        for candidate in row.get("target_candidates", [])
        if candidate.get("all_distinct_values_resolve")
    }
    for method in methods:
        getter_key = f"{method['class']}|{method['method']}"
        if getter_key in wanted_getters:
            getters[getter_key].append(method)
        if method["class"] in wanted_tables and method["method"] == "GetTable":
            table_accessors[str(method["class"])].append(method)
    return getters, table_accessors


def scan_direct_calls(
    binary: Path,
    target_rvas: set[int],
    methods: list[dict[str, object]],
) -> tuple[
    dict[int, list[dict[str, object]]],
    list[dict[str, object]],
    dict[int, list[object]],
]:
    by_method_rva: dict[int, list[dict[str, object]]] = defaultdict(list)
    for method in methods:
        by_method_rva[int(method["rva"])].append(method)
    method_starts = sorted(by_method_rva)
    calls_by_target: dict[int, list[dict[str, object]]] = defaultdict(list)
    section_reports: list[dict[str, object]] = []
    instructions_by_caller: dict[int, list[object]] = {}
    pe = pefile.PE(str(binary), fast_load=True)
    executable_sections: list[dict[str, object]] = []
    raw_candidates_by_caller: dict[int, list[dict[str, int]]] = defaultdict(list)
    for section in pe.sections:
        if not (section.Characteristics & 0x20000000):
            continue
        section_rva = int(section.VirtualAddress)
        data = section.get_data()
        section_name = section.Name.rstrip(b"\0").decode("ascii", errors="replace")
        section_report = {
            "name": section_name,
            "rva": section_rva,
            "bytes": len(data),
            "raw_relative_call_candidates": 0,
            "validated_relative_calls": 0,
        }
        section_reports.append(section_report)
        executable_sections.append(
            {
                "name": section_name,
                "rva": section_rva,
                "end_rva": section_rva + len(data),
                "data": data,
                "report": section_report,
            }
        )

        # This first pass is only a destination prefilter. A raw 0xE8 byte is
        # never accepted as a call until the containing dumped method has been
        # disassembled from its real instruction boundary below.
        offset = data.find(b"\xE8")
        while offset >= 0:
            if offset + 5 <= len(data):
                relative = struct.unpack_from("<i", data, offset + 1)[0]
                call_rva = section_rva + offset
                destination = call_rva + 5 + relative
                if destination in target_rvas:
                    caller_index = bisect.bisect_right(method_starts, call_rva) - 1
                    if caller_index >= 0:
                        caller_start = method_starts[caller_index]
                        raw_candidates_by_caller[caller_start].append(
                            {
                                "call_rva": call_rva,
                                "target_rva": destination,
                            }
                        )
                        section_report["raw_relative_call_candidates"] += 1
            offset = data.find(b"\xE8", offset + 1)

    decoder = Cs(CS_ARCH_X86, CS_MODE_64)
    decoder.detail = True
    decoder.skipdata = True
    for caller_start, raw_candidates in sorted(raw_candidates_by_caller.items()):
        caller_index = bisect.bisect_left(method_starts, caller_start)
        next_start = (
            method_starts[caller_index + 1]
            if caller_index + 1 < len(method_starts)
            else None
        )
        containing_section = next(
            (
                section
                for section in executable_sections
                if int(section["rva"]) <= caller_start < int(section["end_rva"])
            ),
            None,
        )
        if containing_section is None:
            continue
        section_end = int(containing_section["end_rva"])
        method_end = min(next_start if next_start is not None else section_end, section_end)
        if method_end <= caller_start:
            continue
        section_offset = caller_start - int(containing_section["rva"])
        method_data = containing_section["data"][
            section_offset : section_offset + (method_end - caller_start)
        ]
        wanted = {
            (candidate["call_rva"], candidate["target_rva"])
            for candidate in raw_candidates
        }
        decoded_instructions = list(decoder.disasm(method_data, caller_start))
        instructions_by_caller[caller_start] = decoded_instructions
        for instruction in decoded_instructions:
            if instruction.mnemonic != "call" or not instruction.operands:
                continue
            operand = instruction.operands[0]
            if operand.type != X86_OP_IMM:
                continue
            call_rva = int(instruction.address)
            destination = int(operand.imm)
            if (call_rva, destination) not in wanted:
                continue
            calls_by_target[destination].append(
                {
                    "call_rva": call_rva,
                    "target_rva": destination,
                    "caller_start_rva": caller_start,
                    "caller_offset": call_rva - caller_start,
                    "caller_next_method_rva": next_start,
                    "caller_names": [
                        str(item["display_name"])
                        for item in by_method_rva.get(caller_start, [])
                    ],
                }
            )
            containing_section["report"]["validated_relative_calls"] += 1
    for calls in calls_by_target.values():
        calls.sort(key=lambda item: int(item["call_rva"]))
    return calls_by_target, section_reports, instructions_by_caller


def shared_callers(
    source_methods: list[dict[str, object]],
    target_methods_: list[dict[str, object]],
    calls_by_target: dict[int, list[dict[str, object]]],
) -> list[dict[str, object]]:
    source_calls: dict[int, list[dict[str, object]]] = defaultdict(list)
    target_calls: dict[int, list[dict[str, object]]] = defaultdict(list)
    for method in source_methods:
        for call in calls_by_target.get(int(method["rva"]), []):
            if call["caller_start_rva"] is not None:
                source_calls[int(call["caller_start_rva"])].append(call)
    for method in target_methods_:
        for call in calls_by_target.get(int(method["rva"]), []):
            if call["caller_start_rva"] is not None:
                target_calls[int(call["caller_start_rva"])].append(call)
    evidence: list[dict[str, object]] = []
    for caller_rva in sorted(set(source_calls) & set(target_calls)):
        names = sorted(
            {
                name
                for call in source_calls[caller_rva] + target_calls[caller_rva]
                for name in call["caller_names"]
            }
        )
        evidence.append(
            {
                "caller_rva": caller_rva,
                "caller_names": names,
                "source_getter_calls": [call["call_rva"] for call in source_calls[caller_rva]],
                "target_table_calls": [call["call_rva"] for call in target_calls[caller_rva]],
            }
        )
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--candidates", required=True, type=Path)
    parser.add_argument("--game-build", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    candidate_rows = read_jsonl(args.candidates)
    methods = parse_dump(args.dump)
    aliases_by_rva: dict[int, list[str]] = defaultdict(list)
    for method in methods:
        aliases_by_rva[int(method["rva"])].append(str(method["display_name"]))
    aliases_by_rva = {
        rva: sorted(set(names)) for rva, names in aliases_by_rva.items()
    }
    generic_inst_rvas = parse_generic_inst_rvas(args.dump)
    lookup_generic_instances = (
        "ZTable<int, object>.ContainsKey",
        "ZTable<int, object>.TryGetValue",
        "ZTable<int, object>.TryGetValueByLongKey",
        "ZTable<int, object>.GetTableRow",
        "ZTable<int, object>.get_Item",
    )
    lookup_methods_by_rva: dict[int, list[str]] = defaultdict(list)
    for generic_instance in lookup_generic_instances:
        for rva in generic_inst_rvas.get(generic_instance, []):
            lookup_methods_by_rva[int(rva)].append(generic_instance)
    lookup_methods_by_rva = {
        rva: sorted(set(names)) for rva, names in lookup_methods_by_rva.items()
    }
    if not lookup_methods_by_rva:
        raise ValueError("current dump does not expose a supported ZTable<int, object> key lookup")
    getters, accessors = target_methods(methods, candidate_rows)
    selected_methods = [
        method
        for bucket in list(getters.values()) + list(accessors.values())
        for method in bucket
    ]
    target_rvas = {int(method["rva"]) for method in selected_methods}
    calls_by_target, sections, instructions_by_caller = scan_direct_calls(
        args.binary, target_rvas, methods
    )

    results: list[dict[str, object]] = []
    for row in candidate_rows:
        source_key = f"{row['source_table']}Base|get_{row['field']}"
        source_methods = getters.get(source_key, [])
        candidates = []
        for candidate in row.get("target_candidates", []):
            if not candidate.get("all_distinct_values_resolve"):
                continue
            target_class = f"{candidate['target_table']}Base"
            target_methods_ = accessors.get(target_class, [])
            evidence = shared_callers(source_methods, target_methods_, calls_by_target)
            dataflow_proofs: list[dict[str, object]] = []
            for consumer in evidence:
                caller_rva = int(consumer["caller_rva"])
                consumer_proofs = prove_target_lookup_dataflow(
                    instructions_by_caller.get(caller_rva, []),
                    {int(value) for value in consumer["source_getter_calls"]},
                    {int(value) for value in consumer["target_table_calls"]},
                    lookup_methods_by_rva,
                )
                for proof in consumer_proofs:
                    proof["caller_rva"] = caller_rva
                    proof["caller_names"] = consumer["caller_names"]
                dataflow_proofs.extend(consumer_proofs)
            source_rvas = sorted({int(item["rva"]) for item in source_methods})
            target_accessor_rvas = sorted({int(item["rva"]) for item in target_methods_})
            candidates.append(
                {
                    "target_table": candidate["target_table"],
                    "namespace_coverage": {
                        "distinct": candidate.get("distinct_coverage"),
                        "occurrences": candidate.get("occurrence_coverage"),
                    },
                    "name_alignment": candidate.get("name_alignment"),
                    "source_getter_rvas": source_rvas,
                    "source_getter_rva_aliases": {
                        f"0x{rva:X}": aliases_by_rva.get(rva, []) for rva in source_rvas
                    },
                    "target_get_table_rvas": target_accessor_rvas,
                    "target_get_table_rva_aliases": {
                        f"0x{rva:X}": aliases_by_rva.get(rva, [])
                        for rva in target_accessor_rvas
                    },
                    "shared_direct_call_consumers": evidence,
                    "current_build_shared_consumer_corroborated": bool(evidence),
                    "current_build_target_lookup_dataflow_proofs": dataflow_proofs,
                    "current_build_target_lookup_proven": bool(dataflow_proofs),
                }
            )
        corroborated = [
            candidate
            for candidate in candidates
            if candidate["current_build_shared_consumer_corroborated"]
        ]
        proven = [
            candidate
            for candidate in candidates
            if candidate["current_build_target_lookup_proven"]
        ]
        results.append(
            {
                "semantic_field_key": row["semantic_field_key"],
                "source_table": row["source_table"],
                "field": row["field"],
                "path_pattern": row["path_pattern"],
                "candidate_sha256": row.get("candidate_sha256"),
                "full_coverage_candidate_tables": row.get("full_coverage_candidate_tables", 0),
                "source_getter_found": bool(source_methods),
                "candidate_proofs": candidates,
                "corroborated_target_tables": [
                    candidate["target_table"] for candidate in corroborated
                ],
                "proven_target_tables": [candidate["target_table"] for candidate in proven],
                "promotion_state": (
                    "single-current-build-target-lookup-proof"
                    if len(proven) == 1
                    else "ambiguous-current-build-target-lookup-proof"
                    if len(proven) > 1
                    else "shared-consumer-corroborated-not-proven"
                    if corroborated
                    else "not-callsite-corroborated"
                ),
            }
        )

    summary = {
        "candidate_field_groups": len(results),
        "dumped_methods": len(methods),
        "selected_target_methods": len(selected_methods),
        "selected_target_rvas": len(target_rvas),
        "direct_calls_to_selected_targets": sum(len(calls) for calls in calls_by_target.values()),
        "source_getter_found_groups": sum(bool(row["source_getter_found"]) for row in results),
        "shared_consumer_corroborated_groups": sum(
            bool(row["corroborated_target_tables"]) for row in results
        ),
        "single_current_build_target_lookup_proofs": sum(
            row["promotion_state"] == "single-current-build-target-lookup-proof"
            for row in results
        ),
        "ambiguous_current_build_target_lookup_proofs": sum(
            row["promotion_state"] == "ambiguous-current-build-target-lookup-proof"
            for row in results
        ),
        "not_target_lookup_proven_groups": sum(
            not bool(row["proven_target_tables"]) for row in results
        ),
    }
    report = {
        "schema_version": 3,
        "generated_by": "tools/il2cpp-table-reference-callsite-proof.py",
        "game_build": str(args.game_build),
        "inputs": {
            "binary": {"path": str(args.binary.resolve()), "sha256": sha256(args.binary)},
            "dump": {"path": str(args.dump.resolve()), "sha256": sha256(args.dump)},
            "candidates": {"path": str(args.candidates.resolve()), "sha256": sha256(args.candidates)},
            "lookup_methods": {
                "generic_instances": list(lookup_generic_instances),
                "rvas": sorted(lookup_methods_by_rva),
                "methods_by_rva": {
                    f"0x{rva:X}": names
                    for rva, names in sorted(lookup_methods_by_rva.items())
                },
            },
        },
        "policy": {
            "numeric_namespace_membership_alone_is_not_proof": True,
            "field_name_similarity_alone_is_not_proof": True,
            "shared_current_build_direct_call_consumer_is_corroboration_only": True,
            "promotion_requires_source_value_as_target_table_lookup_key": True,
            "promotion_lookup_methods": list(lookup_generic_instances),
            "caller_identity_is_closest_preceding_dumped_method_interval": True,
            "indirect_or_inlined_consumers_are_not_claimed_absent": True,
            "unproven_candidates_are_retained": True,
        },
        "executable_sections": sections,
        "summary": summary,
        "fields": results,
    }
    write_json(args.output, report)
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
