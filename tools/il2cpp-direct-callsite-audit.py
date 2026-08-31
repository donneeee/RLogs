#!/usr/bin/env python3
"""Map direct x64 calls to named IL2CPP methods for a specific client binary.

This is an evidence extractor, not a decompiler. It scans PE executable sections
for relative CALL instructions whose resolved RVA equals a selected method RVA,
then identifies the narrowest enclosing IL2CPP method from Il2CppDumper's
script.json method-address index. Short caller disassemblies are retained so
later proof tooling can validate the actual transform implementation.
"""

from __future__ import annotations

import argparse
import bisect
import hashlib
import json
import re
import struct
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_64, Cs


ADDRESS_RE = re.compile(r'^\s*"Address":\s*(\d+),?\s*$')
NAME_RE = re.compile(r'^\s*"Name":\s*"((?:\\.|[^"\\])*)",?\s*$')
DUMP_RVA_RE = re.compile(r"^\s*// RVA: 0x([0-9A-Fa-f]+)\b")
DUMP_NAMESPACE_RE = re.compile(r"^\s*// Namespace:\s*(.*)\s*$")
DUMP_TYPE_RE = re.compile(
    r"^\s*(?:(?:public|private|protected|internal|static|abstract|sealed|partial)\s+)*"
    r"(?:class|struct|interface)\s+([^\s:<{]+)"
)


def read_method_index(path: Path) -> list[dict[str, object]]:
    methods: list[dict[str, object]] = []
    pending_address: int | None = None
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
                pending_address = int(address_match.group(1))
                continue
            if pending_address is None:
                continue
            name_match = NAME_RE.match(line)
            if name_match:
                name = json.loads(f'"{name_match.group(1)}"')
                methods.append({"address": pending_address, "name": name})
                pending_address = None
    methods.sort(key=lambda item: (int(item["address"]), str(item["name"])))
    return methods


def read_dump_method_index(path: Path) -> list[dict[str, object]]:
    """Read the method RVA/name index directly from Il2CppDumper dump.cs.

    The dump places an exact `// RVA:` line immediately before each method
    declaration. Type and namespace labels are retained only to make caller
    identities auditable; address matching remains the authority.
    """
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


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def containing_methods(
    address: int,
    addresses: list[int],
    by_address: dict[int, list[str]],
) -> dict[str, object]:
    index = bisect.bisect_right(addresses, address) - 1
    if index < 0:
        return {"start_rva": None, "offset": None, "names": []}
    start = addresses[index]
    return {
        "start_rva": start,
        "offset": address - start,
        "names": by_address[start],
    }


def disassemble_window(
    md: Cs,
    section_data: bytes,
    section_rva: int,
    call_rva: int,
    before: int = 48,
    after: int = 48,
    caller_start_rva: int | None = None,
) -> list[dict[str, object]]:
    relative = call_rva - section_rva
    if caller_start_rva is not None and section_rva <= caller_start_rva <= call_rva:
        start = caller_start_rva - section_rva
    else:
        start = max(0, relative - before)
    end = min(len(section_data), relative + 5 + after)
    instructions: list[dict[str, object]] = []
    for instruction in md.disasm(section_data[start:end], section_rva + start):
        instructions.append(
            {
                "rva": instruction.address,
                "size": instruction.size,
                "mnemonic": instruction.mnemonic,
                "operands": instruction.op_str,
                "is_target_call": instruction.address == call_rva,
            }
        )
    return instructions


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    method_index = parser.add_mutually_exclusive_group(required=True)
    method_index.add_argument("--script-json", type=Path)
    method_index.add_argument("--dump", type=Path)
    parser.add_argument("--target", action="append", default=[])
    parser.add_argument(
        "--target-rva",
        action="append",
        default=[],
        type=lambda value: int(value, 0),
        help="exact target RVA; repeat for multiple native targets",
    )
    parser.add_argument("--before", type=int, default=48)
    parser.add_argument("--after", type=int, default=48)
    parser.add_argument(
        "--from-caller-start",
        action="store_true",
        help="start each disassembly at the enclosing method interval start",
    )
    parser.add_argument("--output", type=Path)
    parser.add_argument("--game-build", required=True)
    args = parser.parse_args()

    methods = (
        read_method_index(args.script_json)
        if args.script_json is not None
        else read_dump_method_index(args.dump)
    )
    if not args.target and not args.target_rva:
        parser.error("at least one --target or --target-rva is required")
    if args.before < 0 or args.after < 0:
        parser.error("--before and --after must be non-negative")

    matching = [
        method
        for method in methods
        if any(fragment.casefold() in str(method["name"]).casefold() for fragment in args.target)
    ]
    if args.target and not matching and not args.target_rva:
        raise SystemExit("no selected IL2CPP methods matched")

    by_address: dict[int, list[str]] = {}
    for method in methods:
        by_address.setdefault(int(method["address"]), []).append(str(method["name"]))
    addresses = sorted(by_address)
    target_by_address: dict[int, list[str]] = {}
    for method in matching:
        target_by_address.setdefault(int(method["address"]), []).append(str(method["name"]))
    for target_rva in args.target_rva:
        target_by_address.setdefault(target_rva, by_address.get(target_rva, []))

    pe = pefile.PE(str(args.binary), fast_load=True)
    pe.parse_data_directories()
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    callsites: list[dict[str, object]] = []
    executable_sections = []
    for section in pe.sections:
        if not (section.Characteristics & 0x20000000):
            continue
        section_rva = int(section.VirtualAddress)
        data = section.get_data()
        executable_sections.append(
            {
                "name": section.Name.rstrip(b"\0").decode("ascii", errors="replace"),
                "rva": section_rva,
                "bytes": len(data),
            }
        )
        cursor = 0
        while True:
            offset = data.find(b"\xE8", cursor)
            if offset < 0 or offset + 5 > len(data):
                break
            displacement = struct.unpack_from("<i", data, offset + 1)[0]
            call_rva = section_rva + offset
            destination = call_rva + 5 + displacement
            names = target_by_address.get(destination)
            if names is not None:
                caller = containing_methods(call_rva, addresses, by_address)
                callsites.append(
                    {
                        "call_rva": call_rva,
                        "target_rva": destination,
                        "target_names": names,
                        "caller": caller,
                        "disassembly": disassemble_window(
                            md,
                            data,
                            section_rva,
                            call_rva,
                            before=args.before,
                            after=args.after,
                            caller_start_rva=(
                                caller["start_rva"] if args.from_caller_start else None
                            ),
                        ),
                    }
                )
            cursor = offset + 1

    report = {
        "schema_version": 3,
        "generated_by": "rlogs-il2cpp-direct-callsite-audit",
        "game_build": args.game_build,
        "binary": {
            "path": str(args.binary.resolve()),
            "byte_length": args.binary.stat().st_size,
            "sha256": sha256(args.binary),
        },
        "method_index": {
            "path": str((args.script_json or args.dump).resolve()),
            "format": "script-json" if args.script_json is not None else "dump-cs",
            "method_entries": len(methods),
        },
        "targets": [
            {"rva": address, "names": names}
            for address, names in sorted(target_by_address.items())
        ],
        "executable_sections": executable_sections,
        "callsites": callsites,
        "summary": {
            "selected_method_names": len(matching),
            "selected_exact_target_rvas": len(set(args.target_rva)),
            "unique_target_rvas": len(target_by_address),
            "direct_callsites": len(callsites),
            "named_caller_callsites": sum(bool(callsite["caller"]["names"]) for callsite in callsites),
        },
        "policy": {
            "direct_call_match_is_exact": True,
            "containing_method_is_address_interval_inference": True,
            "indirect_calls_are_not_claimed_absent": True,
            "formula_semantics_require_instruction_level_validation": True,
        },
    }
    encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(json.dumps(report["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
