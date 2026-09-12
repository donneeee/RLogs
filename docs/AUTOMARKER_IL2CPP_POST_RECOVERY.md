# Automarker IL2CPP post-recovery pipeline

This is the immediate offline pipeline after a complete build-25247556
`global-metadata.dat` has been recovered. It does not authorize process access,
game modification, or network transmission.

## Extraction boundary

The repository can validate exact inputs, parse Il2CppDumper output, route named
methods to native RVAs/VAs, and perform bounded PE disassembly. It cannot derive
IL2CPP registrations or managed identities directly from the static
`global-metadata.dat` + `GameAssembly.dll` pair. An external compatible IL2CPP
extractor is therefore required to produce `dump.cs` (and preferably
`script.json`). Run that extractor outside the repository against the exact pair;
do not substitute an older build's output.

### Pinned extractor source

Use only the official `Perfare/Il2CppDumper` source. The latest numbered release
is `v6.7.46` at commit
`8a521b9c180cf13499253f0818cbc729dca767cb`; current `master` is the single
follow-up commit `4741d46ba9cd6159c5d853eb9d6fc48b4bfa2b1a`, which changes the project
targets from `net6.0;net7.0` to `net6.0;net8.0`. Pin the full current-source
commit instead of building a moving branch:

```powershell
git clone --filter=blob:none --no-checkout https://github.com/Perfare/Il2CppDumper.git <private>\Il2CppDumper-src
git -C <private>\Il2CppDumper-src fetch --depth 1 origin 4741d46ba9cd6159c5d853eb9d6fc48b4bfa2b1a
git -C <private>\Il2CppDumper-src checkout --detach 4741d46ba9cd6159c5d853eb9d6fc48b4bfa2b1a
git -C <private>\Il2CppDumper-src rev-parse HEAD
dotnet publish <private>\Il2CppDumper-src\Il2CppDumper\Il2CppDumper.csproj `
  -c Release -f net8.0 -r win-x64 --self-contained true `
  --output <private>\Il2CppDumper-publish
```

The source is MIT licensed. Its project has one NuGet dependency,
`Mono.Cecil` `0.11.4`, and no checked-in package lock file; `dotnet publish`
therefore performs a network restore unless an already-reviewed package cache
and source configuration are supplied. Retain the source commit, NuGet source,
resolved package hash, published-file hashes, extractor `config.json`, and input
identity receipt with the private outputs. The official tag is lightweight (not
an annotated signed tag), the repository has no `SECURITY.md`, and the project
does not publish a reproducible-build attestation, so a source build reduces but
does not eliminate supply-chain risk.

On the audited Windows x64 host, .NET SDK `10.0.400` is installed but a .NET 8
runtime is not. The self-contained `net8.0` publish above avoids depending on a
machine-wide .NET 8 runtime. The upstream `config.json` is copied beside the
published executable; keep `GenerateStruct: true` to emit `script.json` (plus
`stringliteral.json` and `il2cpp.h`), and set `RequireAnyKey: false` for a
non-interactive offline run. `dump.cs` is emitted independently. Create the
private output directory before invoking the documented three-argument command,
because the program recognizes the third argument as an output directory only
when it already exists.

Compatibility remains conditional until the recovered metadata header is
validated: this revision accepts metadata versions 16 through 31. A valid pair
must use the exact recovered `global-metadata.dat` and the receipt-bound
build-25247556 `GameAssembly.dll`; do not carry forward either file or generated
output from another build.

Required chain:

1. Retain the recovered metadata and its schema-2 exact identity receipt
   privately. The router requires the receipt and verifies that it binds the
   metadata, GameAssembly, process executable, Steam manifest, app, and build.
2. Run an external IL2CPP extractor with the recovered metadata and exact
   build-25247556 `GameAssembly.dll`; retain `dump.cs` and `script.json` together.
3. Run the repository router below. It pins the GameAssembly digest, validates
   the metadata header, hashes all inputs, checks dump VAs against the PE image
   base, and emits candidates for `PlayerInputController.FlagSkill`,
   `TouchController.TrySetAxis`, the `World.UseSlot`/async bridge, and targeting
   or raycast names.
4. Treat targeting/raycast rows as candidates only. Prove the handler through a
   bounded native call graph/disassembly from `FlagSkill` toward `World.UseSlot`;
   names alone do not establish call ordering or position ownership.

```powershell
node tools/bpsr-automarker-il2cpp-route.mjs build `
  --build 25247556 `
  --metadata "<private>\global-metadata.dat" `
  --identity "<private>\client-binary-identity.json" `
  --game-assembly "<install>\bpsr\GameAssembly.dll" `
  --dump "<private>\il2cpp-25247556\dump.cs" `
  --output "<private>\automarker-il2cpp-route.v1.json"
```

For direct-call evidence after reviewing the routed RVAs:

```powershell
python tools/il2cpp-direct-callsite-audit.py `
  --binary "<install>\bpsr\GameAssembly.dll" `
  --dump "<private>\il2cpp-25247556\dump.cs" `
  --target "PlayerInputController.FlagSkill" `
  --target "TouchController.TrySetAxis" `
  --target "UseSlot" `
  --game-build 25247556 `
  --output "<private>\automarker-direct-calls.v1.json"
```

The existing Python native analyzers require `pefile` and `capstone`. The Node
router uses only Node built-ins. Neither tool should write into the repository
when the recovered metadata or outputs are private.

Test the routing/parser contract with:

```powershell
node tools/bpsr-automarker-il2cpp-route.mjs self-test
```
