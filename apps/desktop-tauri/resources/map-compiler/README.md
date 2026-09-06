# Local BPSR map compiler

Release builds place `rlogs-bpsr-map-compiler.exe` here before Tauri packages
the installer. The helper reads map textures only from the user's own installed
BPSR client and writes them only to rLogs' local runtime-data directory. Extracted
game assets are never bundled, committed, or uploaded.

The helper is built from `tools/bpsr-local-map-asset.py` with the exact package
versions in `tools/bpsr-map-compiler-requirements.txt`. UnityPy and its runtime
dependencies retain their respective upstream licenses; PyInstaller's bootloader
is distributed under its GPL exception for bundled applications.
