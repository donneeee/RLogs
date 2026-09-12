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
