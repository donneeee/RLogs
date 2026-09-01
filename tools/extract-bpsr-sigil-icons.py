#!/usr/bin/env python3
"""Extract every localized sigil texture from an installed BPSR Steam client."""

from __future__ import annotations

import io
import json
import os
import re
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import UnityPy


ROOT = Path(__file__).resolve().parents[1]
ITEM_TABLE = ROOT / "tmp-rdps-audit/external/BPSR-ZDPS/BPSR-ZDPS/Data/ItemTable.json"
DEFAULT_CONTAINER = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common\Blue Protocol Star Resonance"
    r"\bpsr\BPSR_STEAM_Data\StreamingAssets\container"
)
OUTPUT = ROOT / "assets/blue-protocol-star-resonance/shared/icons/profile/equipment"
NAME_PATTERN = re.compile(br"item_icons_enchantformula[0-9]+", re.IGNORECASE)


def extract_package(arguments: tuple[str, tuple[str, ...]]) -> list[tuple[str, bytes]]:
    package_name, target_names = arguments
    package = Path(package_name)
    data = package.read_bytes()
    names = {match.decode("ascii").lower() for match in NAME_PATTERN.findall(data)}
    wanted = names.intersection(target_names)
    if not wanted:
        return []

    starts: list[int] = []
    offset = 0
    while True:
        offset = data.find(b"UnityFS\x00", offset)
        if offset < 0:
            break
        starts.append(offset)
        offset += 8

    extracted: list[tuple[str, bytes]] = []
    for index, start in enumerate(starts):
        end = starts[index + 1] if index + 1 < len(starts) else len(data)
        bundle = data[start:end]
        if not any(name.encode("ascii") in bundle.lower() for name in wanted):
            continue
        try:
            environment = UnityPy.load(bundle)
        except Exception:
            continue
        for obj in environment.objects:
            if obj.type.name != "Texture2D":
                continue
            texture = obj.read()
            name = getattr(texture, "m_Name", "").lower()
            if name not in wanted:
                continue
            stream = io.BytesIO()
            texture.image.save(stream, format="PNG")
            extracted.append((name, stream.getvalue()))
    return extracted


def main() -> None:
    container = Path(os.environ.get("BPSR_CONTAINER_DIR", DEFAULT_CONTAINER))
    items = json.loads(ITEM_TABLE.read_text(encoding="utf-8"))
    targets = tuple(sorted({
        item["Icon"].lower()
        for item in items.values()
        if item.get("Type") == 102
        and "sigil" in item.get("Name", "").lower()
        and item.get("Icon")
    }))
    packages = [str(path) for path in container.glob("m*.pkg") if path.name != "m0.pkg"]
    OUTPUT.mkdir(parents=True, exist_ok=True)

    extracted: dict[str, bytes] = {}
    workers = min(16, os.cpu_count() or 1)
    with ProcessPoolExecutor(max_workers=workers) as executor:
        for result in executor.map(extract_package, ((package, targets) for package in packages)):
            for name, png in result:
                extracted.setdefault(name, png)

    for name, png in sorted(extracted.items()):
        (OUTPUT / f"{name}.png").write_bytes(png)

    missing = sorted(set(targets).difference(extracted))
    print(f"extracted {len(extracted)} of {len(targets)} unique sigil textures with {workers} workers")
    if missing:
        raise SystemExit(f"missing sigil textures: {', '.join(missing)}")


if __name__ == "__main__":
    main()
