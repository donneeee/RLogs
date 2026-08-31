#!/usr/bin/env python3
"""Recover exact protobuf field tags from current IL2CPP MergeFrom methods.

This is an offline, build-locked research tool. It never treats generated
field order as a protobuf tag. Instead it joins each native wire-key branch to
the exact IL2CPP instance-field offset written by that branch.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import defaultdict
from pathlib import Path

try:
    import pefile
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs
    from capstone.x86 import (
        X86_OP_IMM,
        X86_OP_MEM,
        X86_OP_REG,
        X86_REG_EAX,
        X86_REG_ECX,
        X86_REG_RAX,
        X86_REG_RCX,
        X86_REG_R8,
    )
except ImportError as error:  # pragma: no cover - environment error path
    raise SystemExit(
        "protobuf wire proof requires the offline research packages pefile and capstone"
    ) from error


TYPE_RE = re.compile(r"^(?:public|private|internal|protected).*? (class|struct) ([^ :]+)")
FIELD_RE = re.compile(r"^\s*(?!.*\bstatic\b).*?\s+([^\s;]+);\s*// 0x([0-9A-Fa-f]+)$")
SIMPLE_TYPED_FIELD_RE = re.compile(
    r"^\s*(?!.*\bstatic\b)(?:public|private|internal|protected)\s+"
    r"(?:readonly\s+)?([^\s;]+)\s+([^\s;]+);\s*// 0x([0-9A-Fa-f]+)$"
)
RVA_RE = re.compile(r"^\s*// RVA: 0x([0-9A-Fa-f]+)")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def identity(path: Path) -> dict:
    return {"byte_length": path.stat().st_size, "sha256": sha256(path)}


def parse_dump(path: Path) -> dict[str, dict]:
    namespace = ""
    active: dict | None = None
    pending_rva: int | None = None
    classes: dict[str, dict] = {}

    with path.open("r", encoding="utf-8-sig", errors="strict") as source:
        for line in source:
            stripped = line.strip()
            if stripped.startswith("// Namespace: "):
                namespace = stripped.removeprefix("// Namespace: ")
                continue
            type_match = TYPE_RE.match(stripped)
            if type_match:
                short_name = type_match.group(2)
                full_name = f"{namespace}.{short_name}" if namespace else short_name
                active = {
                    "full_name": full_name,
                    "kind": type_match.group(1),
                    "fields": {},
                    "field_types": {},
                    "merge_from_rva": None,
                }
                classes[full_name] = active
                pending_rva = None
                continue
            if active is None:
                continue
            if stripped == "}":
                active = None
                pending_rva = None
                continue
            rva_match = RVA_RE.match(line)
            if rva_match:
                pending_rva = int(rva_match.group(1), 16)
                continue
            field_match = FIELD_RE.match(line)
            if field_match:
                active["fields"][int(field_match.group(2), 16)] = field_match.group(1)
                typed_match = SIMPLE_TYPED_FIELD_RE.match(line)
                if typed_match:
                    active["field_types"][int(typed_match.group(3), 16)] = typed_match.group(1)
                continue
            if "InternalMergeFrom(ref ParseContext input" in stripped:
                active["merge_from_rva"] = pending_rva
            pending_rva = None
    return classes


def method_instructions(pe, assembly: Path, rva: int):
    file_offset = pe.get_offset_from_rva(rva)
    with assembly.open("rb") as source:
        source.seek(file_offset)
        data = source.read(0x10000)
    decoder = Cs(CS_ARCH_X86, CS_MODE_64)
    decoder.detail = True
    instructions = []
    image_base = pe.OPTIONAL_HEADER.ImageBase
    for instruction in decoder.disasm(data, image_base + rva):
        instructions.append(instruction)
        if instruction.mnemonic == "ret":
            break
    return instructions, image_base


def register_written_from_rcx(instruction) -> int | None:
    if instruction.mnemonic != "mov" or len(instruction.operands) != 2:
        return None
    destination, source = instruction.operands
    if destination.type != X86_OP_REG or source.type != X86_OP_REG:
        return None
    if source.reg != X86_REG_RCX:
        return None
    return destination.reg


def register_written_from_r8(instruction) -> int | None:
    if instruction.mnemonic != "mov" or len(instruction.operands) != 2:
        return None
    destination, source = instruction.operands
    if destination.type != X86_OP_REG or source.type != X86_OP_REG:
        return None
    if source.reg != X86_REG_R8:
        return None
    return destination.reg


def field_accesses(instructions, register: int, offsets: set[int]) -> int:
    return sum(
        1
        for instruction in instructions
        for operand in instruction.operands
        if operand.type == X86_OP_MEM
        and operand.mem.base == register
        and operand.mem.disp in offsets
    )


def prove_message(pe, assembly: Path, generated: dict, dumped: dict | None) -> dict:
    if dumped is None:
        return unresolved_message(generated, "message_absent_from_il2cpp_dump")
    if dumped["merge_from_rva"] is None:
        return unresolved_message(generated, "native_internal_merge_from_rva_absent")
    if not generated["fields"]:
        return {
            "full_name": generated["full_name"],
            "internal_merge_from_rva_hex": f"0x{dumped['merge_from_rva']:X}",
            "field_count": 0,
            "exact_field_tags": 0,
            "state": "exact",
            "proof_reason": "exact_empty_message",
            "ambiguous_branches": [],
            "fields": [],
        }

    generated_names = {field["name"] for field in generated["fields"]}
    dump_fields = {
        offset: name for offset, name in dumped["fields"].items() if name in generated_names
    }
    missing_offsets = sorted(generated_names - set(dump_fields.values()))
    if missing_offsets:
        result = unresolved_message(generated, "generated_fields_absent_from_dump_offsets")
        result["missing_dump_fields"] = missing_offsets
        return result

    instructions, image_base = method_instructions(
        pe, assembly, dumped["merge_from_rva"]
    )
    candidates = {
        register_written_from_rcx(instruction)
        for instruction in instructions[:80]
        if register_written_from_rcx(instruction) is not None
    }
    if not candidates:
        return unresolved_message(generated, "native_object_register_not_found")
    object_register = max(
        candidates,
        key=lambda register: field_accesses(instructions, register, set(dump_fields)),
    )

    keys_by_offset: dict[int, set[int]] = defaultdict(set)
    ambiguous_branches = []
    instruction_index = {instruction.address: index for index, instruction in enumerate(instructions)}
    for index, instruction in enumerate(instructions[:-1]):
        if instruction.mnemonic != "cmp" or len(instruction.operands) != 2:
            continue
        left, right = instruction.operands
        if (
            left.type != X86_OP_REG
            or left.reg not in (X86_REG_EAX, X86_REG_ECX)
            or right.type != X86_OP_IMM
        ):
            continue
        branch = instructions[index + 1]
        if branch.mnemonic != "jne" or not branch.operands or branch.operands[0].type != X86_OP_IMM:
            continue
        target = branch.operands[0].imm
        if target <= branch.address:
            continue
        offsets = set()
        for candidate in instructions[index + 2 :]:
            if candidate.address >= target:
                break
            for operand in candidate.operands:
                if (
                    operand.type == X86_OP_MEM
                    and operand.mem.base == object_register
                    and operand.mem.disp in dump_fields
                ):
                    offsets.add(operand.mem.disp)
            # Generated parsers leave every recognized-field block through a
            # direct jump to the common loop tail. Linear disassembly beyond
            # that jump belongs to a different comparison branch.
            if candidate.mnemonic in ("jmp", "ret"):
                break
        if len(offsets) == 1:
            keys_by_offset[next(iter(offsets))].add(right.imm)
        elif len(offsets) > 1:
            ambiguous_branches.append(
                {
                    "comparison_rva_hex": f"0x{instruction.address - image_base:X}",
                    "wire_key_decimal": right.imm,
                    "candidate_field_offsets_hex": [f"0x{value:X}" for value in sorted(offsets)],
                }
            )

    # Repeated primitive fields accept both unpacked and packed encodings. The
    # generated parser commonly folds those two keys into:
    #   add eax, -<unpacked-key>; test eax, <mask>; je <field-block>
    # Recover both accepted keys and join the shared target block to its exact
    # instance-field offset.
    for index, instruction in enumerate(instructions[:-2]):
        if instruction.mnemonic not in ("add", "lea") or len(instruction.operands) != 2:
            continue
        transform_left, transform_right = instruction.operands
        test = instructions[index + 1]
        branch = instructions[index + 2]
        if instruction.mnemonic == "add":
            transform_ok = (
                transform_left.type == X86_OP_REG
                and transform_left.reg in (X86_REG_EAX, X86_REG_ECX)
                and transform_right.type == X86_OP_IMM
            )
            transformed_register = transform_left.reg
            add_value = transform_right.imm & 0xFFFFFFFF
        else:
            transform_ok = (
                transform_left.type == X86_OP_REG
                and transform_left.reg in (X86_REG_EAX, X86_REG_ECX)
                and transform_right.type == X86_OP_MEM
                and transform_right.mem.base
                in (X86_REG_EAX, X86_REG_ECX, X86_REG_RAX, X86_REG_RCX)
                and transform_right.mem.index == 0
            )
            transformed_register = transform_left.reg
            add_value = transform_right.mem.disp & 0xFFFFFFFF
        if (
            not transform_ok
            or test.mnemonic != "test"
            or len(test.operands) != 2
            or test.operands[0].type != X86_OP_REG
            or test.operands[0].reg != transformed_register
            or test.operands[1].type != X86_OP_IMM
            or branch.mnemonic not in ("je", "jne")
            or not branch.operands
            or branch.operands[0].type != X86_OP_IMM
        ):
            continue
        target_index = (
            instruction_index.get(branch.operands[0].imm)
            if branch.mnemonic == "je"
            else index + 3
        )
        if target_index is None:
            continue
        offsets = set()
        for candidate in instructions[target_index:]:
            for operand in candidate.operands:
                if (
                    operand.type == X86_OP_MEM
                    and operand.mem.base == object_register
                    and operand.mem.disp in dump_fields
                ):
                    offsets.add(operand.mem.disp)
            if candidate.mnemonic in ("jmp", "ret"):
                break
        if len(offsets) != 1:
            continue
        mask = test.operands[1].imm & 0xFFFFFFFF
        accepted = [
            key
            for key in range(1, 0x4000)
            if (((key + add_value) & 0xFFFFFFFF) & mask) == 0
        ]
        if 1 <= len(accepted) <= 2 and len({key >> 3 for key in accepted}) == 1:
            keys_by_offset[next(iter(offsets))].update(accepted)

    fields = []
    exact = 0
    for generated_field in generated["fields"]:
        offset = next(
            offset for offset, name in dump_fields.items() if name == generated_field["name"]
        )
        keys = sorted(keys_by_offset.get(offset, set()))
        tags = sorted({key >> 3 for key in keys if key > 0 and key & 7 <= 5})
        state = "exact_native_merge_branch" if len(tags) == 1 else "unresolved_native_branch"
        if state == "exact_native_merge_branch":
            exact += 1
        fields.append(
            {
                "order": generated_field["order"],
                "name": generated_field["name"],
                "field_type": generated_field["field_type"],
                "instance_offset_hex": f"0x{offset:X}",
                "protobuf_tag": tags[0] if len(tags) == 1 else None,
                "accepted_wire_keys_decimal": keys,
                "accepted_wire_types": sorted({key & 7 for key in keys}),
                "proof_state": state,
            }
        )
    return {
        "full_name": generated["full_name"],
        "internal_merge_from_rva_hex": f"0x{dumped['merge_from_rva']:X}",
        "field_count": len(fields),
        "exact_field_tags": exact,
        "state": "exact" if exact == len(fields) else "incomplete",
        "ambiguous_branches": ambiguous_branches,
        "fields": fields,
    }


def prove_value_struct(
    pe, assembly: Path, generated: dict, dumped: dict | None, processor: dict | None
) -> dict:
    if dumped is None or dumped.get("kind") != "struct":
        return unresolved_message(generated, "value_struct_absent_from_il2cpp_dump")
    if processor is None or processor["merge_from_rva"] is None:
        return unresolved_message(generated, "value_struct_processor_merge_rva_absent")
    generated_names = {field["name"] for field in generated["fields"]}
    dump_fields = {
        offset: name for offset, name in dumped["fields"].items() if name in generated_names
    }
    missing_offsets = sorted(generated_names - set(dump_fields.values()))
    if missing_offsets:
        result = unresolved_message(generated, "value_struct_fields_absent_from_dump_offsets")
        result["missing_dump_fields"] = missing_offsets
        return result

    instructions, image_base = method_instructions(
        pe, assembly, processor["merge_from_rva"]
    )
    candidates = {
        register_written_from_r8(instruction)
        for instruction in instructions[:40]
        if register_written_from_r8(instruction) is not None
    }
    if not candidates:
        return unresolved_message(generated, "value_struct_destination_register_not_found")
    destination_register = max(
        candidates,
        key=lambda register: field_accesses(instructions, register, set(dump_fields)),
    )
    keys_by_offset: dict[int, set[int]] = defaultdict(set)
    ambiguous_branches = []
    for index, instruction in enumerate(instructions[:-1]):
        if instruction.mnemonic != "cmp" or len(instruction.operands) != 2:
            continue
        left, right = instruction.operands
        if (
            left.type != X86_OP_REG
            or left.reg not in (X86_REG_EAX, X86_REG_ECX)
            or right.type != X86_OP_IMM
        ):
            continue
        branch = instructions[index + 1]
        if (
            branch.mnemonic != "jne"
            or not branch.operands
            or branch.operands[0].type != X86_OP_IMM
        ):
            continue
        target = branch.operands[0].imm
        if target <= branch.address:
            continue
        offsets = set()
        for candidate in instructions[index + 2 :]:
            if candidate.address >= target:
                break
            for operand in candidate.operands:
                if (
                    operand.type == X86_OP_MEM
                    and operand.mem.base == destination_register
                    and operand.mem.disp in dump_fields
                ):
                    offsets.add(operand.mem.disp)
            if candidate.mnemonic in ("jmp", "ret"):
                break
        if len(offsets) == 1:
            keys_by_offset[next(iter(offsets))].add(right.imm)
        elif len(offsets) > 1:
            ambiguous_branches.append(
                {
                    "comparison_rva_hex": f"0x{instruction.address - image_base:X}",
                    "wire_key_decimal": right.imm,
                    "candidate_field_offsets_hex": [
                        f"0x{value:X}" for value in sorted(offsets)
                    ],
                }
            )

    fields = []
    exact = 0
    for generated_field in generated["fields"]:
        offset = next(
            offset for offset, name in dump_fields.items() if name == generated_field["name"]
        )
        keys = sorted(keys_by_offset.get(offset, set()))
        tags = sorted({key >> 3 for key in keys if key > 0 and key & 7 <= 5})
        state = (
            "exact_native_value_struct_processor_branch"
            if len(tags) == 1
            else "unresolved_native_value_struct_processor_branch"
        )
        if len(tags) == 1:
            exact += 1
        fields.append(
            {
                "order": generated_field["order"],
                "name": generated_field["name"],
                "field_type": generated_field["field_type"],
                "instance_offset_hex": f"0x{offset:X}",
                "protobuf_tag": tags[0] if len(tags) == 1 else None,
                "accepted_wire_keys_decimal": keys,
                "accepted_wire_types": sorted({key & 7 for key in keys}),
                "proof_state": state,
            }
        )
    return {
        "full_name": generated["full_name"],
        "internal_merge_from_rva_hex": f"0x{processor['merge_from_rva']:X}",
        "processor_full_name": processor["full_name"],
        "field_count": len(fields),
        "exact_field_tags": exact,
        "state": "exact" if exact == len(fields) else "incomplete",
        "ambiguous_branches": ambiguous_branches,
        "fields": fields,
    }


def unresolved_message(generated: dict, reason: str) -> dict:
    return {
        "full_name": generated["full_name"],
        "internal_merge_from_rva_hex": None,
        "field_count": len(generated["fields"]),
        "exact_field_tags": 0,
        "state": "unresolved",
        "reason": reason,
        "ambiguous_branches": [],
        "fields": [
            {
                "order": field["order"],
                "name": field["name"],
                "field_type": field["field_type"],
                "instance_offset_hex": None,
                "protobuf_tag": None,
                "accepted_wire_keys_decimal": [],
                "accepted_wire_types": [],
                "proof_state": "unresolved_native_branch",
            }
            for field in generated["fields"]
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--surface", type=Path, required=True)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--game-assembly", type=Path, required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--root", action="append", default=[])
    parser.add_argument("--value-struct", action="append", default=[])
    arguments = parser.parse_args()

    surface = json.loads(arguments.surface.read_text(encoding="utf-8"))
    build_identity = json.loads(arguments.identity.read_text(encoding="utf-8"))
    observed_assembly = identity(arguments.game_assembly)
    expected_assembly = build_identity["game_assembly"]
    if (
        observed_assembly["byte_length"] != expected_assembly["byte_length"]
        or observed_assembly["sha256"].lower() != expected_assembly["sha256"].lower()
    ):
        raise SystemExit("GameAssembly does not match the supplied exact-build identity")
    if surface["build_id"] != build_identity["game_build"]:
        raise SystemExit("RPC surface build does not match the supplied exact-build identity")

    roots = set(arguments.root)
    selected = [
        message
        for message in surface["messages"]
        if not roots or message["full_name"] in roots
    ]
    if roots and roots != {message["full_name"] for message in selected}:
        missing = sorted(roots - {message["full_name"] for message in selected})
        raise SystemExit(f"requested root messages absent from RPC surface: {missing}")

    dumped = parse_dump(arguments.dump)
    value_structs = []
    for requested_name in arguments.value_struct:
        full_name = (
            requested_name if "." in requested_name else f"Zproto.{requested_name}"
        )
        dumped_struct = dumped.get(full_name)
        if dumped_struct is None or dumped_struct.get("kind") != "struct":
            raise SystemExit(f"requested value struct absent from IL2CPP dump: {full_name}")
        fields = []
        for order, (offset, name) in enumerate(
            sorted(dumped_struct["fields"].items()), start=1
        ):
            field_type = dumped_struct["field_types"].get(offset)
            if field_type is None:
                raise SystemExit(
                    f"value struct field type unresolved for {full_name}.{name} at 0x{offset:X}"
                )
            fields.append({"order": order, "name": name, "field_type": field_type})
        short_name = full_name.rsplit(".", 1)[-1]
        processor_name = f"Zproto.{short_name}Processor"
        value_structs.append(
            {
                "generated": {"full_name": full_name, "fields": fields},
                "dumped": dumped_struct,
                "processor": dumped.get(processor_name),
            }
        )
    pe = pefile.PE(str(arguments.game_assembly), fast_load=True)
    messages = [
        prove_message(pe, arguments.game_assembly, message, dumped.get(message["full_name"]))
        for message in selected
    ]
    messages.extend(
        prove_value_struct(
            pe,
            arguments.game_assembly,
            value_struct["generated"],
            value_struct["dumped"],
            value_struct["processor"],
        )
        for value_struct in value_structs
    )
    exact_fields = sum(message["exact_field_tags"] for message in messages)
    total_fields = sum(message["field_count"] for message in messages)
    report = {
        "schema_version": 2,
        "generated_by": "rlogs-bpsr-protobuf-native-wire-proof",
        "game": surface["game"],
        "deployment": surface["deployment"],
        "channel": surface["channel"],
        "game_build": surface["build_id"],
        "source_identity": {
            "metadata": build_identity["metadata"],
            "game_assembly": observed_assembly,
            "rpc_surface": identity(arguments.surface),
            "il2cpp_dump": identity(arguments.dump),
        },
        "policy": {
            "offline_research_only": True,
            "exact_build_only": True,
            "generated_field_order_used_as_tag": False,
            "native_merge_branch_and_instance_offset_required": True,
            "native_value_struct_processor_branch_and_offset_required": True,
            "unresolved_fields_hidden": False,
            "packet_replay_required_for_semantics_and_occurrence": True,
        },
        "summary": {
            "messages_requested": len(messages),
            "messages_exact": sum(message["state"] == "exact" for message in messages),
            "messages_incomplete_or_unresolved": sum(message["state"] != "exact" for message in messages),
            "fields_requested": total_fields,
            "exact_field_tags": exact_fields,
            "unresolved_field_tags": total_fields - exact_fields,
        },
        "messages": messages,
    }
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        f"proved {exact_fields}/{total_fields} native protobuf tags across "
        f"{len(messages)} messages; output {arguments.output}"
    )


if __name__ == "__main__":
    main()
