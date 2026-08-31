#!/usr/bin/env python3
"""Inventory constants in Lua 5.3 binary chunks without executing game code.

The BPSR client ships stripped and non-stripped Lua 5.3 chunks. Numeric IDs are
encoded as binary constants, so text search cannot find them. This tool parses
the portable chunk structure, reports functions that contain requested integer
or text constants, and includes neighboring constants as static evidence.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from dataclasses import dataclass, field
from pathlib import Path
from typing import BinaryIO, Iterable


LUA_SIGNATURE = b"\x1bLua"
LUA_VERSION_53 = 0x53
LUA_TNIL = 0
LUA_TBOOLEAN = 1
LUA_TNUMFLT = 3
LUA_TSHRSTR = 4
LUA_TLNGSTR = 20
LUA_TNUMINT = 19


class ChunkError(RuntimeError):
    pass


@dataclass
class ChunkLayout:
    endian: str
    int_size: int
    size_t_size: int
    instruction_size: int
    integer_size: int
    number_size: int


@dataclass
class FunctionRecord:
    source: str | None
    line_defined: int
    last_line_defined: int
    num_params: int
    vararg_flag: int
    max_stack_size: int
    constants: list[object] = field(default_factory=list)
    instructions: list[int] = field(default_factory=list)
    line_info: list[int] = field(default_factory=list)
    upvalues: list[tuple[int, int]] = field(default_factory=list)
    upvalue_names: list[str | None] = field(default_factory=list)
    local_variables: list[tuple[str | None, int, int]] = field(default_factory=list)
    children: list["FunctionRecord"] = field(default_factory=list)


class ChunkReader:
    def __init__(self, handle: BinaryIO):
        self.handle = handle
        self.layout: ChunkLayout | None = None

    def read_exact(self, count: int) -> bytes:
        value = self.handle.read(count)
        if len(value) != count:
            raise ChunkError(f"unexpected EOF: wanted {count} bytes, got {len(value)}")
        return value

    def read_byte(self) -> int:
        return self.read_exact(1)[0]

    def read_uint(self, size: int) -> int:
        assert self.layout is not None
        return int.from_bytes(self.read_exact(size), self.layout.endian, signed=False)

    def read_int(self, size: int | None = None) -> int:
        assert self.layout is not None
        width = size or self.layout.int_size
        return int.from_bytes(self.read_exact(width), self.layout.endian, signed=True)

    def read_number(self) -> float:
        assert self.layout is not None
        formats = {4: "f", 8: "d"}
        fmt = formats.get(self.layout.number_size)
        if fmt is None:
            raise ChunkError(f"unsupported lua_Number size {self.layout.number_size}")
        prefix = "<" if self.layout.endian == "little" else ">"
        return struct.unpack(prefix + fmt, self.read_exact(self.layout.number_size))[0]

    def read_string(self) -> str | None:
        assert self.layout is not None
        size = self.read_byte()
        if size == 0:
            return None
        if size == 0xFF:
            size = self.read_uint(self.layout.size_t_size)
        if size < 1:
            raise ChunkError(f"invalid Lua string size {size}")
        raw = self.read_exact(size - 1)
        return raw.decode("utf-8", errors="replace")

    def read_count(self, label: str) -> int:
        count = self.read_int()
        if count < 0 or count > 50_000_000:
            raise ChunkError(f"invalid {label} count {count}")
        return count

    def read_header(self) -> int:
        if self.read_exact(4) != LUA_SIGNATURE:
            raise ChunkError("not a Lua binary chunk")
        version = self.read_byte()
        if version != LUA_VERSION_53:
            raise ChunkError(f"unsupported Lua bytecode version 0x{version:02x}")
        _format = self.read_byte()
        if self.read_exact(6) != b"\x19\x93\r\n\x1a\n":
            raise ChunkError("invalid LUAC_DATA")
        int_size = self.read_byte()
        size_t_size = self.read_byte()
        instruction_size = self.read_byte()
        integer_size = self.read_byte()
        number_size = self.read_byte()
        raw_integer = self.read_exact(integer_size)
        raw_number = self.read_exact(number_size)

        little_integer = int.from_bytes(raw_integer, "little", signed=True)
        big_integer = int.from_bytes(raw_integer, "big", signed=True)
        if little_integer == 0x5678:
            endian = "little"
        elif big_integer == 0x5678:
            endian = "big"
        else:
            raise ChunkError("cannot determine Lua chunk byte order")
        self.layout = ChunkLayout(
            endian=endian,
            int_size=int_size,
            size_t_size=size_t_size,
            instruction_size=instruction_size,
            integer_size=integer_size,
            number_size=number_size,
        )

        prefix = "<" if endian == "little" else ">"
        fmt = {4: "f", 8: "d"}.get(number_size)
        if fmt is None:
            raise ChunkError(f"unsupported lua_Number size {number_size}")
        luac_number = struct.unpack(prefix + fmt, raw_number)[0]
        if not math.isclose(luac_number, 370.5):
            raise ChunkError(f"unexpected LUAC_NUM {luac_number}")
        return self.read_byte()  # main closure upvalue count

    def read_function(self, inherited_source: str | None = None) -> FunctionRecord:
        assert self.layout is not None
        source = self.read_string() or inherited_source
        line_defined = self.read_int()
        last_line_defined = self.read_int()
        num_params = self.read_byte()
        vararg_flag = self.read_byte()
        max_stack_size = self.read_byte()

        code_count = self.read_count("instruction")
        instructions = [
            self.read_uint(self.layout.instruction_size) for _ in range(code_count)
        ]

        constants: list[object] = []
        for _ in range(self.read_count("constant")):
            tag = self.read_byte()
            if tag == LUA_TNIL:
                constants.append(None)
            elif tag == LUA_TBOOLEAN:
                constants.append(bool(self.read_byte()))
            elif tag == LUA_TNUMFLT:
                constants.append(self.read_number())
            elif tag == LUA_TNUMINT:
                constants.append(self.read_int(self.layout.integer_size))
            elif tag in (LUA_TSHRSTR, LUA_TLNGSTR):
                constants.append(self.read_string())
            else:
                raise ChunkError(f"unsupported constant tag {tag}")

        upvalue_count = self.read_count("upvalue")
        upvalues = [(self.read_byte(), self.read_byte()) for _ in range(upvalue_count)]

        children = [
            self.read_function(source) for _ in range(self.read_count("prototype"))
        ]

        line_info_count = self.read_count("line info")
        line_info = [self.read_int() for _ in range(line_info_count)]
        local_variables = [
            (self.read_string(), self.read_int(), self.read_int())
            for _ in range(self.read_count("local variable"))
        ]
        upvalue_names = [
            self.read_string() for _ in range(self.read_count("upvalue name"))
        ]

        return FunctionRecord(
            source,
            line_defined,
            last_line_defined,
            num_params,
            vararg_flag,
            max_stack_size,
            constants,
            instructions,
            line_info,
            upvalues,
            upvalue_names,
            local_variables,
            children,
        )


LUA53_OPCODES = [
    "MOVE", "LOADK", "LOADKX", "LOADBOOL", "LOADNIL", "GETUPVAL",
    "GETTABUP", "GETTABLE", "SETTABUP", "SETUPVAL", "SETTABLE", "NEWTABLE",
    "SELF", "ADD", "SUB", "MUL", "MOD", "POW", "DIV", "IDIV", "BAND",
    "BOR", "BXOR", "SHL", "SHR", "UNM", "BNOT", "NOT", "LEN", "CONCAT",
    "JMP", "EQ", "LT", "LE", "TEST", "TESTSET", "CALL", "TAILCALL",
    "RETURN", "FORLOOP", "FORPREP", "TFORCALL", "TFORLOOP", "SETLIST",
    "CLOSURE", "VARARG", "EXTRAARG",
]


def describe_rk(value: int, constants: list[object]) -> str:
    if value & 0x100:
        index = value & 0xFF
        rendered = render_constant(constants[index]) if index < len(constants) else "<missing>"
        return f"K{index}={rendered!r}"
    return f"R{value}"


def disassemble(record: FunctionRecord) -> list[dict]:
    rows: list[dict] = []
    for pc, instruction in enumerate(record.instructions):
        opcode = instruction & 0x3F
        a = (instruction >> 6) & 0xFF
        c = (instruction >> 14) & 0x1FF
        b = (instruction >> 23) & 0x1FF
        bx = (instruction >> 14) & 0x3FFFF
        sbx = bx - 131071
        ax = instruction >> 6
        name = LUA53_OPCODES[opcode] if opcode < len(LUA53_OPCODES) else f"OP_{opcode}"
        operands: dict[str, object] = {"A": a, "B": b, "C": c}
        if name == "LOADK":
            operands = {"A": a, "Bx": bx, "constant": render_constant(record.constants[bx]) if bx < len(record.constants) else "<missing>"}
        elif name in {"LOADKX", "CLOSURE"}:
            operands = {"A": a, "Bx": bx}
        elif name == "EXTRAARG":
            operands = {"Ax": ax}
        elif name in {"JMP", "FORLOOP", "FORPREP", "TFORLOOP"}:
            operands = {"A": a, "sBx": sbx, "target_pc": pc + 1 + sbx}
        elif name in {"GETTABUP", "GETTABLE", "SETTABUP", "SETTABLE", "SELF", "ADD", "SUB", "MUL", "MOD", "POW", "DIV", "IDIV", "BAND", "BOR", "BXOR", "SHL", "SHR", "EQ", "LT", "LE"}:
            operands["B_ref"] = describe_rk(b, record.constants)
            operands["C_ref"] = describe_rk(c, record.constants)
        rows.append(
            {
                "pc": pc,
                "line": record.line_info[pc] if pc < len(record.line_info) else None,
                "opcode": name,
                "operands": operands,
                "raw": f"0x{instruction:08x}",
            }
        )
    return rows


def flatten(record: FunctionRecord, path: tuple[int, ...] = ()) -> Iterable[tuple[tuple[int, ...], FunctionRecord]]:
    yield path, record
    for index, child in enumerate(record.children):
        yield from flatten(child, path + (index,))


def parse_targets(values: list[str]) -> tuple[set[int], list[str]]:
    integers: set[int] = set()
    strings: list[str] = []
    for value in values:
        try:
            integers.add(int(value, 0))
        except ValueError:
            strings.append(value.casefold())
    return integers, strings


def render_constant(value: object) -> object:
    if isinstance(value, float) and not math.isfinite(value):
        return repr(value)
    return value


def audit_file(path: Path, integer_targets: set[int], string_targets: list[str], include_disassembly: bool = False, include_all_functions: bool = False) -> dict:
    with path.open("rb") as handle:
        reader = ChunkReader(handle)
        main_upvalues = reader.read_header()
        root = reader.read_function()
        trailing = handle.read()
    matches = []
    for function_path, record in flatten(root):
        integer_hits = sorted(
            value
            for value in record.constants
            if isinstance(value, int) and not isinstance(value, bool) and value in integer_targets
        )
        string_hits = sorted(
            {
                target
                for target in string_targets
                if any(
                    isinstance(value, str) and target in value.casefold()
                    for value in record.constants
                )
            }
        )
        if not include_all_functions and not integer_hits and not string_hits:
            continue
        match = {
                "function_path": list(function_path),
                "source": record.source,
                "line_defined": record.line_defined,
                "last_line_defined": record.last_line_defined,
                "num_params": record.num_params,
                "vararg_flag": record.vararg_flag,
                "max_stack_size": record.max_stack_size,
                "upvalues": [
                    {
                        "name": record.upvalue_names[index] if index < len(record.upvalue_names) else None,
                        "instack": descriptor[0],
                        "index": descriptor[1],
                    }
                    for index, descriptor in enumerate(record.upvalues)
                ],
                "local_variables": [
                    {"name": value[0], "start_pc": value[1], "end_pc": value[2]}
                    for value in record.local_variables
                ],
                "integer_hits": integer_hits,
                "string_hits": string_hits,
                "constants": [render_constant(value) for value in record.constants],
            }
        if include_disassembly:
            match["instructions"] = disassemble(record)
        matches.append(match)
    return {
        "file": str(path.resolve()),
        "main_upvalues": main_upvalues,
        "trailing_bytes": len(trailing),
        "matches": matches,
    }


def iter_chunks(roots: list[Path]) -> Iterable[Path]:
    seen: set[Path] = set()
    for root in roots:
        paths = [root] if root.is_file() else root.rglob("*")
        for path in paths:
            if not path.is_file() or path.suffix.casefold() not in {".lua", ".luac"}:
                continue
            resolved = path.resolve()
            if resolved in seen:
                continue
            seen.add(resolved)
            yield path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("roots", nargs="+", type=Path)
    parser.add_argument("--target", action="append", default=[], help="exact integer or case-insensitive string fragment")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--disassemble", action="store_true", help="include decoded Lua 5.3 instructions for matching functions")
    parser.add_argument("--all-functions", action="store_true", help="include every parsed function; requires --disassemble or still emits metadata/constants")
    args = parser.parse_args()
    integer_targets, string_targets = parse_targets(args.target)
    if not integer_targets and not string_targets:
        parser.error("at least one --target is required")

    reports = []
    failures = []
    scanned = 0
    for path in iter_chunks(args.roots):
        scanned += 1
        try:
            report = audit_file(path, integer_targets, string_targets, args.disassemble, args.all_functions)
        except (OSError, ChunkError, UnicodeError) as error:
            failures.append({"file": str(path.resolve()), "error": str(error)})
            continue
        if report["matches"]:
            reports.append(report)

    result = {
        "schema_version": 1,
        "generated_by": "tools/lua53-constant-audit.py",
        "policy": {
            "executes_game_code": False,
            "integer_matches_are_exact": True,
            "string_matches_are_case_insensitive_fragments": True,
            "neighbor_constants_are_evidence_not_automatic_semantics": True,
        },
        "targets": {"integers": sorted(integer_targets), "strings": string_targets},
        "summary": {
            "files_scanned": scanned,
            "files_with_matches": len(reports),
            "functions_with_matches": sum(len(report["matches"]) for report in reports),
            "parse_failures": len(failures),
        },
        "files": reports,
        "failures": failures,
    }
    encoded = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
