#!/usr/bin/env python3
"""Prove the build-locked client skill-action attribute envelope.

IL2CPP metadata method-pointer tables can drift while the managed metadata stays
byte-identical.  This offline tool therefore treats dump method labels only as
the starting point for the UseSlot async state machine.  It discovers the
skill-attribute boundary by its three exact entity-attribute reads, proves the
plaintext wire keys from the called native function, and verifies the envelope
keys directly in the exact decrypted metadata bytes.

The report is static evidence only.  It never authorizes a packet route without
matching-build packet replay.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

try:
    import pefile
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs
    from capstone.x86 import X86_OP_IMM
except ImportError as error:  # pragma: no cover - environment error path
    raise SystemExit(
        "native skill-attribute proof requires the offline research packages "
        "pefile and capstone"
    ) from error


ATTRIBUTE_IDS = (11_720, 11_730, 11_740)
PLAINTEXT_WIRE_KEYS = (0x08, 0x15, 0x18, 0x20, 0x28)
SERVICE_HASH_MULTIPLIER = 131
SERVICE_HASH_MASK = 0x7FFF_FFFF
USE_SLOT_STATE_RE = re.compile(
    r"private struct WorldProxy\.<UseSlot>d__\d+.*?"
    r"// RVA: 0x([0-9A-Fa-f]+).*?private void MoveNext\(\)",
    re.DOTALL,
)
WORLD_PROXY_UUID_RE = re.compile(
    r"public class WorldProxy : ZProxy, IWorldProxy.*?"
    r"// RVA: 0x([0-9A-Fa-f]+).*?public override ulong Uuid\(\)",
    re.DOTALL,
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def identity(path: Path) -> dict:
    return {"byte_length": path.stat().st_size, "sha256": sha256(path)}


def validate_identity(name: str, observed: dict, expected: dict) -> None:
    if (
        observed["byte_length"] != expected["byte_length"]
        or observed["sha256"].lower() != expected["sha256"].lower()
    ):
        raise SystemExit(f"{name} does not match the supplied exact-build identity")


def bkdr_131_service_id(name: str) -> int:
    """Return the protocol's unsigned BKDR-131 service id."""
    value = 0
    for byte in name.encode("utf-8"):
        value = ((value * SERVICE_HASH_MULTIPLIER) + byte) & 0xFFFF_FFFF
    return value & SERVICE_HASH_MASK


def prove_service_hash_contract(rpc_surface: dict, build_identity: dict) -> dict:
    if str(rpc_surface.get("build_id")) != str(build_identity["game_build"]):
        raise SystemExit("RPC surface does not match the supplied exact game build")
    if rpc_surface.get("game") != build_identity["game"]:
        raise SystemExit("RPC surface game does not match the supplied build identity")
    if rpc_surface.get("deployment") != build_identity["deployment"]:
        raise SystemExit("RPC surface deployment does not match the supplied build identity")
    if rpc_surface.get("channel") != build_identity["channel"]:
        raise SystemExit("RPC surface channel does not match the supplied build identity")

    rpc_identity = rpc_surface.get("source_identity", {})
    validate_identity(
        "RPC-surface GameAssembly",
        rpc_identity.get("game_assembly", {}),
        build_identity["game_assembly"],
    )
    validate_identity(
        "RPC-surface decrypted metadata",
        rpc_identity.get("metadata", {}),
        build_identity["metadata"],
    )

    validations = []
    for service in rpc_surface.get("services", []):
        if service.get("id_state") != "exact_native_factory_return":
            continue
        observed = service.get("service_id")
        if observed is None:
            raise SystemExit("exact native RPC service has no numeric service id")
        expected = bkdr_131_service_id(service["name"])
        if observed != expected:
            raise SystemExit(
                f"RPC service {service['name']} id {observed} does not match "
                f"BKDR-131 masked id {expected}"
            )
        validations.append(
            {
                "service": service["name"],
                "native_service_id_decimal": observed,
                "native_service_id_hex": f"0x{observed:X}",
                "derived_service_id_decimal": expected,
                "derived_service_id_hex": f"0x{expected:X}",
                "proof_state": "exact_native_factory_return_matches_hash",
            }
        )

    if len(validations) < 3:
        raise SystemExit(
            "RPC surface has fewer than three exact native service ids; refusing "
            "to establish the build-scoped service hash contract"
        )

    world_service_id = bkdr_131_service_id("World")
    return {
        "algorithm": "BKDR-131 over UTF-8 bytes, unsigned 32-bit wrap, high bit masked",
        "multiplier": SERVICE_HASH_MULTIPLIER,
        "mask_hex": f"0x{SERVICE_HASH_MASK:X}",
        "exact_native_validation_count": len(validations),
        "exact_native_validations": validations,
        "derived_world_service_id_decimal": world_service_id,
        "derived_world_service_id_hex": f"0x{world_service_id:X}",
        "proof_state": "exact_current_build_native_validated_service_name_hash",
    }


def parse_function_ranges(pe) -> tuple[list[int], dict[int, int]]:
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXCEPTION"]]
    )
    ranges = {
        entry.struct.BeginAddress: entry.struct.EndAddress
        for entry in pe.DIRECTORY_ENTRY_EXCEPTION
        if entry.struct.EndAddress > entry.struct.BeginAddress
    }
    starts = sorted(ranges)
    if not starts:
        raise SystemExit("GameAssembly has no x64 runtime-function table")
    return starts, ranges


def containing_function(starts: list[int], ends: dict[int, int], rva: int) -> int | None:
    index = bisect.bisect_right(starts, rva) - 1
    if index < 0:
        return None
    start = starts[index]
    return start if rva < ends[start] else None


def disassemble_function(pe, assembly_bytes: bytes, start: int, end: int):
    offset = pe.get_offset_from_rva(start)
    decoder = Cs(CS_ARCH_X86, CS_MODE_64)
    decoder.detail = True
    return list(
        decoder.disasm(
            assembly_bytes[offset : offset + end - start],
            pe.OPTIONAL_HEADER.ImageBase + start,
        )
    )


def direct_call_target(instruction, image_base: int) -> int | None:
    if (
        instruction.mnemonic != "call"
        or len(instruction.operands) != 1
        or instruction.operands[0].type != X86_OP_IMM
    ):
        return None
    target = instruction.operands[0].imm - image_base
    return target if target >= 0 else None


def immediate_values(instructions) -> list[int]:
    return [
        operand.imm
        for instruction in instructions
        for operand in instruction.operands
        if operand.type == X86_OP_IMM
    ]


def find_attribute_boundary(pe, assembly_bytes: bytes, starts, ends):
    candidate_sets = []
    for attribute_id in ATTRIBUTE_IDS:
        pattern = b"\xBA" + attribute_id.to_bytes(4, "little")
        candidates = set()
        offset = 0
        while True:
            offset = assembly_bytes.find(pattern, offset)
            if offset < 0:
                break
            try:
                rva = pe.get_rva_from_offset(offset)
            except pefile.PEFormatError:
                offset += 1
                continue
            function = containing_function(starts, ends, rva)
            if function is not None:
                candidates.add(function)
            offset += 1
        candidate_sets.append(candidates)
    functions = set.intersection(*candidate_sets)
    ordered_functions = []
    for function in functions:
        instructions = disassemble_function(pe, assembly_bytes, function, ends[function])
        ordered_reads = tuple(
            instruction.operands[1].imm
            for instruction in instructions
            if instruction.mnemonic == "mov"
            and len(instruction.operands) == 2
            and instruction.operands[1].type == X86_OP_IMM
            and instruction.operands[1].imm in ATTRIBUTE_IDS
        )
        if ordered_reads == ATTRIBUTE_IDS:
            ordered_functions.append(function)
    if len(ordered_functions) != 1:
        raise SystemExit(
            "expected exactly one native function reading attributes in exact envelope "
            f"order {ATTRIBUTE_IDS}; found "
            f"{[hex(value) for value in sorted(ordered_functions)]}; all unordered candidates "
            f"were {[hex(value) for value in sorted(functions)]}"
        )
    return ordered_functions[0]


def prove_attribute_reads(instructions, image_base: int) -> tuple[list[dict], int]:
    reads = []
    for index, instruction in enumerate(instructions):
        if instruction.mnemonic != "mov" or len(instruction.operands) != 2:
            continue
        source = instruction.operands[1]
        if source.type != X86_OP_IMM or source.imm not in ATTRIBUTE_IDS:
            continue
        call = next(
            (
                candidate
                for candidate in instructions[index + 1 : index + 4]
                if direct_call_target(candidate, image_base) is not None
            ),
            None,
        )
        if call is None:
            raise SystemExit(f"attribute {source.imm} is not followed by a direct getter call")
        reads.append(
            {
                "attribute_id": source.imm,
                "instruction_rva_hex": f"0x{instruction.address - image_base:X}",
                "getter_rva_hex": f"0x{direct_call_target(call, image_base):X}",
            }
        )
    if tuple(read["attribute_id"] for read in reads) != ATTRIBUTE_IDS:
        raise SystemExit(f"attribute read order is not exact: {reads}")
    getters = {read["getter_rva_hex"] for read in reads}
    if len(getters) != 1:
        raise SystemExit(f"speed attributes use different native getters: {sorted(getters)}")
    return reads, next(i for i, instruction in enumerate(instructions) if instruction.address - image_base == int(reads[-1]["instruction_rva_hex"], 16))


def ordered_subsequence(values: list[int], expected: tuple[int, ...]) -> bool:
    position = 0
    for value in values:
        if position < len(expected) and value == expected[position]:
            position += 1
    return position == len(expected)


def discover_called_contracts(
    instructions, after_index: int, image_base: int, pe, assembly_bytes: bytes, starts, ends
) -> tuple[int, int, list[int], dict]:
    calls = []
    for instruction in instructions[after_index + 1 :]:
        target = direct_call_target(instruction, image_base)
        if target is None or target not in ends:
            continue
        callee = disassemble_function(pe, assembly_bytes, target, ends[target])
        byte_immediates = [
            operand.imm
            for item in callee
            for operand in item.operands
            if operand.type == X86_OP_IMM and 0 <= operand.imm <= 0xFF
        ]
        calls.append((instruction, target, callee, byte_immediates))

    builders = [call for call in calls if ordered_subsequence(call[3], PLAINTEXT_WIRE_KEYS)]
    if len(builders) != 1:
        raise SystemExit(
            "expected exactly one called plaintext builder with wire keys "
            f"{PLAINTEXT_WIRE_KEYS}; found {[hex(call[1]) for call in builders]}"
        )
    builder_call, builder_rva, _, builder_immediates = builders[0]
    builder_position = instructions.index(builder_call)
    following_calls = [
        item
        for item in instructions[builder_position + 1 :]
        if direct_call_target(item, image_base) is not None
        and direct_call_target(item, image_base) in ends
    ]
    if not following_calls:
        raise SystemExit("plaintext builder has no following direct envelope call")
    encrypt_call = following_calls[0]
    encrypt_rva = direct_call_target(encrypt_call, image_base)
    encrypt_instructions = disassemble_function(pe, assembly_bytes, encrypt_rva, ends[encrypt_rva])
    encrypt_immediates = Counter(immediate_values(encrypt_instructions))
    missing = [value for value in (16, 32, 48) if encrypt_immediates[value] == 0]
    if missing:
        raise SystemExit(
            f"called envelope function 0x{encrypt_rva:X} lacks structural sizes {missing}"
        )
    return (
        builder_rva,
        encrypt_rva,
        [value for value in builder_immediates if value in PLAINTEXT_WIRE_KEYS],
        {
            "iv_size_occurrences": encrypt_immediates[16],
            "hmac_size_occurrences": encrypt_immediates[32],
            "envelope_prefix_size_occurrences": encrypt_immediates[48],
        },
    )


def prove_world_service_id(
    dump_text: str,
    pe,
    assembly_bytes: bytes,
    starts,
    ends,
    service_hash_proof: dict,
) -> dict:
    match = WORLD_PROXY_UUID_RE.search(dump_text)
    if not match:
        raise SystemExit("IL2CPP dump has no WorldProxy.Uuid method")
    uuid_rva = int(match.group(1), 16)
    resolved_rva = uuid_rva
    jump_chain = []
    for _ in range(8):
        offset = pe.get_offset_from_rva(resolved_rva)
        body = assembly_bytes[offset : offset + 16]
        if len(body) < 5 or body[0] != 0xE9:
            break
        displacement = int.from_bytes(body[1:5], "little", signed=True)
        target_rva = resolved_rva + 5 + displacement
        jump_chain.append(
            {
                "from_rva_hex": f"0x{resolved_rva:X}",
                "to_rva_hex": f"0x{target_rva:X}",
                "instruction": "jmp_rel32",
            }
        )
        resolved_rva = target_rva
    else:
        raise SystemExit("WorldProxy.Uuid native jump chain exceeds eight thunks")

    offset = pe.get_offset_from_rva(resolved_rva)
    body = assembly_bytes[offset : offset + 16]
    if len(body) < 6:
        raise SystemExit("WorldProxy.Uuid native body is truncated")
    if body[0] == 0xB8 and body[5] == 0xC3:
        service_id = int.from_bytes(body[1:5], "little")
        instruction_shape = "mov_eax_imm32_ret"
    elif len(body) >= 11 and body[0:2] == b"\x48\xB8" and body[10] == 0xC3:
        service_id = int.from_bytes(body[2:10], "little")
        instruction_shape = "mov_rax_imm64_ret"
    else:
        service_id = None
        instruction_shape = "virtualized_native_body"
    if service_id == 0:
        raise SystemExit("WorldProxy.Uuid returned the invalid zero service id")
    native_service_id = service_id
    derived_service_id = service_hash_proof["derived_world_service_id_decimal"]
    if native_service_id is not None and native_service_id != derived_service_id:
        raise SystemExit(
            "WorldProxy.Uuid native constant disagrees with the current-build "
            "native-validated service-name hash contract"
        )
    service_id = derived_service_id
    runtime_function = containing_function(starts, ends, resolved_rva)
    return {
        "service_id_decimal": service_id,
        "service_id_hex": f"0x{service_id:X}",
        "service_id_source": (
            "exact_native_proxy_uuid_constant_return_leaf"
            if native_service_id is not None
            else "exact_current_build_native_validated_service_name_hash"
        ),
        "native_uuid_service_id_decimal": native_service_id,
        "native_uuid_service_id_hex": (
            f"0x{native_service_id:X}" if native_service_id is not None else None
        ),
        "dump_uuid_label_rva_hex": f"0x{uuid_rva:X}",
        "native_jump_chain": jump_chain,
        "resolved_native_body_rva_hex": f"0x{resolved_rva:X}",
        "native_instruction_shape": instruction_shape,
        "native_body_prefix_hex": body[:11].hex(),
        "runtime_function_table_entry": (
            f"0x{runtime_function:X}" if runtime_function is not None else None
        ),
        "native_uuid_proof_state": (
            "exact_native_proxy_uuid_constant_return_leaf"
            if native_service_id is not None
            else "exact_native_proxy_uuid_virtualized_current_service_id_unresolved"
        ),
        "proof_state": (
            "exact_native_proxy_uuid_constant_return_leaf"
            if native_service_id is not None
            else "exact_current_build_native_validated_service_name_hash"
        ),
    }


def prove_use_slot_route(
    dump_text: str,
    pe,
    assembly_bytes: bytes,
    starts,
    ends,
    service_hash_proof: dict,
) -> dict:
    service = prove_world_service_id(
        dump_text, pe, assembly_bytes, starts, ends, service_hash_proof
    )
    match = USE_SLOT_STATE_RE.search(dump_text)
    if not match:
        raise SystemExit("IL2CPP dump has no WorldProxy.<UseSlot> MoveNext method")
    move_next_rva = int(match.group(1), 16)
    function = containing_function(starts, ends, move_next_rva)
    if function != move_next_rva:
        raise SystemExit(
            f"dumped UseSlot MoveNext RVA 0x{move_next_rva:X} is not an exact function start"
        )
    instructions = disassemble_function(pe, assembly_bytes, function, ends[function])
    candidates = Counter(
        value
        for value in immediate_values(instructions)
        if 0x10000 <= value <= 0xFFFFF
    )
    repeated = [(value, count) for value, count in candidates.items() if count >= 2]
    if len(repeated) != 1:
        raise SystemExit(f"UseSlot method-id proof is ambiguous: {repeated}")
    method_id, occurrences = repeated[0]
    return {
        "service": "World",
        **service,
        "method": "UseSlot",
        "move_next_rva_hex": f"0x{move_next_rva:X}",
        "method_id_decimal": method_id,
        "method_id_hex": f"0x{method_id:X}",
        "native_immediate_occurrences": occurrences,
        "method_id_proof_state": "exact_native_async_request_method_id",
    }


def prove_metadata_keys(metadata: Path, prior_contract: dict) -> dict:
    metadata_bytes = metadata.read_bytes()
    prior_metadata = prior_contract["source_binaries"]["decrypted_global_metadata"]
    observed = identity(metadata)
    expected = {
        "byte_length": prior_metadata["length"],
        "sha256": prior_metadata["sha256"],
    }
    validate_identity("decrypted metadata continuity", observed, expected)
    output = {}
    for name in ("aes_key", "hmac_key"):
        contract = prior_contract["authenticated_envelope"][name]
        offset = int(contract["metadata_default_value_offset_hex"], 16)
        expected_bytes = bytes.fromhex(contract["hex"])
        observed_bytes = metadata_bytes[offset : offset + len(expected_bytes)]
        if observed_bytes != expected_bytes:
            raise SystemExit(f"{name} metadata default no longer matches the prior exact contract")
        output[name] = {
            "hex": observed_bytes.hex(),
            "metadata_default_value_offset_hex": f"0x{offset:X}",
            "proof_state": "exact_bytes_in_byte_identical_decrypted_metadata",
        }
    return {"metadata": observed, **output}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--game-assembly", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--rpc-surface", type=Path, required=True)
    parser.add_argument("--prior-contract", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()

    build_identity = json.loads(arguments.identity.read_text(encoding="utf-8"))
    rpc_surface = json.loads(arguments.rpc_surface.read_text(encoding="utf-8"))
    prior_contract = json.loads(arguments.prior_contract.read_text(encoding="utf-8"))
    observed_assembly = identity(arguments.game_assembly)
    validate_identity("GameAssembly", observed_assembly, build_identity["game_assembly"])
    observed_metadata = identity(arguments.metadata)
    validate_identity("decrypted metadata", observed_metadata, build_identity["metadata"])
    service_hash_proof = prove_service_hash_contract(rpc_surface, build_identity)

    assembly_bytes = arguments.game_assembly.read_bytes()
    pe = pefile.PE(str(arguments.game_assembly), fast_load=True)
    starts, ends = parse_function_ranges(pe)
    boundary_rva = find_attribute_boundary(pe, assembly_bytes, starts, ends)
    boundary = disassemble_function(pe, assembly_bytes, boundary_rva, ends[boundary_rva])
    attribute_reads, final_attribute_index = prove_attribute_reads(
        boundary, pe.OPTIONAL_HEADER.ImageBase
    )
    builder_rva, encrypt_rva, builder_keys, envelope_sizes = discover_called_contracts(
        boundary,
        final_attribute_index,
        pe.OPTIONAL_HEADER.ImageBase,
        pe,
        assembly_bytes,
        starts,
        ends,
    )
    dump_text = arguments.dump.read_text(encoding="utf-8-sig", errors="strict")
    route = prove_use_slot_route(
        dump_text, pe, assembly_bytes, starts, ends, service_hash_proof
    )
    key_proof = prove_metadata_keys(arguments.metadata, prior_contract)

    report = {
        "schema_version": 3,
        "generated_by": "rlogs-bpsr-use-skill-attribute-native-proof",
        "game": build_identity["game"],
        "deployment": build_identity["deployment"],
        "channel": build_identity["channel"],
        "game_build": build_identity["game_build"],
        "source_identity": {
            "game_assembly": observed_assembly,
            "metadata": observed_metadata,
            "il2cpp_dump": identity(arguments.dump),
            "rpc_message_surface": identity(arguments.rpc_surface),
            "prior_contract": identity(arguments.prior_contract),
        },
        "policy": {
            "offline_research_only": True,
            "exact_build_only": True,
            "dump_label_used_as_attribute_boundary_authority": False,
            "native_signature_required": True,
            "unresolved_evidence_hidden": False,
            "matching_build_packet_replay_required": True,
            "runtime_route_authorized": False,
        },
        "service_hash_contract": service_hash_proof,
        "route": route,
        "attribute_boundary": {
            "rva_hex": f"0x{boundary_rva:X}",
            "end_rva_hex": f"0x{ends[boundary_rva]:X}",
            "attribute_reads": attribute_reads,
            "plaintext_builder_rva_hex": f"0x{builder_rva:X}",
            "encrypt_envelope_rva_hex": f"0x{encrypt_rva:X}",
            "plaintext_wire_keys": builder_keys,
            "plaintext_field_numbers": [key >> 3 for key in builder_keys],
            "envelope_structural_sizes": envelope_sizes,
            "proof_state": "exact_native_signature",
        },
        "authenticated_envelope_key_continuity": key_proof,
        "promotion_state": {
            "static_action_contract_exact": True,
            "current_build_service_id_exact": route["service_id_decimal"] is not None,
            "complete_static_route_exact": route["service_id_decimal"] is not None,
            "matching_build_packet_replay_exact": False,
            "runtime_route_enabled": False,
        },
    }
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        f"proved World.UseSlot method {route['method_id_hex']} with service-id state "
        f"{route['proof_state']}, action boundary "
        f"0x{boundary_rva:X}, builder 0x{builder_rva:X}, envelope 0x{encrypt_rva:X}; "
        f"output {arguments.output}"
    )


if __name__ == "__main__":
    main()
