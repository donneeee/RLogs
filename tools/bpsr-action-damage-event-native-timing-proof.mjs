#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  createReadStream,
  existsSync,
  openSync,
  readFileSync,
  readSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 11;
const EXPECTED_BUILD = "24687926";
const EXPECTED_DEPLOYMENT = "global";
const EXPECTED_BINARY_BYTES = 217_629_232;
const EXPECTED_BINARY_SHA256 =
  "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";
const EXPECTED_BATTLE_FRAME_REFERENCE_AUDIT_SHA256 =
  "2a340ec858418611497e7e05e7300e4c2fde6fb52fb05cdb9baf55c62ba730e4";
const EXPECTED_BATTLE_FRAME_SETTER_CALLSITE_AUDIT_SHA256 =
  "a0ea930aca0c6733cbe6a8b7f4a099395c4d08b82c688a0001f0781da62f49ad";
const GAME_CONTEXT_TYPE_INFO_RVA = 0x95d1998;
const LOGIC_FRAME_RATE = 30;

const METHOD_REGIONS = [
  {
    name: "StageEventData numeric-event orchestrator (conflicting dump label parseHitForOffset)",
    rva: 0x5274730,
    end_rva: 0x5275ae0,
    sha256: "b066c057551e23ee9008626317c629247535225b1901974a922febc6af88687e",
  },
  {
    name: "StageEventData runtime event-dictionary constructor (conflicting dump label parseHitForHit)",
    rva: 0x5275ae0,
    end_rva: 0x5276280,
    sha256: "efaf5cbaf268095845f9438c95805e75d118e9258612f736b8f93aa5dfbcdee5",
  },
  {
    name: "native-standard-HitData-timing-parser (conflicting dump label parseHit)",
    rva: 0x5276750,
    end_rva: 0x5277310,
    sha256: "3fec1d9fbc412b7b17a8ade835a4412a707d2555ebc1a34d442b37190166c4be",
  },
  {
    name: "native-offset/common-HitData-timing-parser",
    rva: 0x5276280,
    end_rva: 0x5276750,
    sha256: "ae73d4205b22fbacb0219329bdc752bb5e0884a8d202b2917bada654b6404839",
  },
  {
    name: "Panda.Core.GameContext..cctor",
    rva: 0x3f31740,
    end_rva: 0x3f31cd6,
    sha256: "51943f1ef2b594af86b574884d8674e5b5e77f76682624f035ef1fcdde3352db",
  },
  {
    name: "native standard-versus-common HitData parser wrapper (conflicting dump label .ctor)",
    rva: 0x5278520,
    end_rva: 0x5278840,
    sha256: "b6278c8f942c0491627e934ea6775b2ebd488f01d61668ee278661263de9981e",
  },
  {
    name: "DetectBeHit native wrapper (script interval label conflicts with public ABI)",
    rva: 0x52801d0,
    end_rva: 0x5280e90,
    sha256: "c6db23ad205ba125b39afa91d2b8a200c0c1a652ad7a4c2af66478b729985d82",
  },
  {
    name: "DetectHit native wrapper (script interval label conflicts with public ABI)",
    rva: 0x5280e90,
    end_rva: 0x5281c80,
    sha256: "d5ac9d055dc42d2746a39e0741b9fee8d8c22935d3300bb9f81192b870c96005",
  },
  {
    name: "HitData timing scheduler consumer (conflicting dump label prePlayBeHit)",
    rva: 0x5281f60,
    end_rva: 0x5282cb0,
    sha256: "368a579014c1ae6b6787c1ddcc190f1f34f104229e0995b65357257b87db2f08",
  },
  {
    name: "CtrlSkill outgoing-hit TimeFactor caller path",
    rva: 0x547afa0,
    end_rva: 0x547b690,
    sha256: "38d209fe27feabcf4ac91f1c76ac84a975a9d76ce8dc1a69de837bacb6e73c54",
  },
  {
    name: "CtrlSkill stage-exit action-speed formula-to-component path",
    rva: 0x547a0a0,
    end_rva: 0x547afa0,
    sha256: "8c3ec6f2a3dfda2166c0a45db14171441872f45b6d44b59eff1ebf20374c50be",
  },
  {
    name: "action-speed component float setter",
    rva: 0x4131040,
    end_rva: 0x4131190,
    sha256: "a92f5f690b06b3ba353595dda15ebcff96216254685744506c6125f7f65df657",
  },
  {
    name: "action-speed component float getter",
    rva: 0x4131890,
    end_rva: 0x4131940,
    sha256: "e2b7cb35d111031b4c7aded5850f886153affa3fd07d64a3fab1004cd14685f3",
  },
  {
    name: "companion speed-factor float getter",
    rva: 0x4154390,
    end_rva: 0x4154440,
    sha256: "c2bb77674649e1c5f98fa5fe0b387f482264f911f62e636403be0d9b0e9f0eff",
  },
  {
    name: "CtrlSkill component factors-to-LogicRuntimeBlockData.TimeFactor path",
    rva: 0x5479ec0,
    end_rva: 0x5479f50,
    sha256: "ed0356857307061f65a7560b51139afef07c42c1832fbf46c6560be71bda7149",
  },
  {
    name: "LogicRuntimeBlock constructor and stage ownership path",
    rva: 0x51de250,
    end_rva: 0x51de570,
    sha256: "f822c8e513afb490e00d3f2452c14d21a737f42aa5a30de73e05478416048a04",
  },
  {
    name: "LogicRuntimeBlock before-stage-after update dispatcher (conflicting dump label ForceSwitchStage)",
    rva: 0x51de720,
    end_rva: 0x51de9e0,
    sha256: "5b4a634735be84e7c886bc80e91f59e8a40bc6a821de764a7542c916432fa63b",
  },
  {
    name: "LogicStage constructor retaining its LogicRuntimeBlock (conflicting dump label Update)",
    rva: 0x51dfb00,
    end_rva: 0x51dfd20,
    sha256: "4f5d175f874a74c9b2d1cd47606a3fc9b897f9a9eb7a23ffeb7c89a24d11a979",
  },
  {
    name: "LogicStage enter lifecycle (conflicting dump label Exit)",
    rva: 0x51dfd20,
    end_rva: 0x51e01c0,
    sha256: "f24cd03a95df423b9cf27ee2036daeb4236efaabc8c0935d43bc8b2fe5dbb3db",
  },
  {
    name: "LogicStage update-list and stage-callback dispatcher (conflicting dump label forceEnd)",
    rva: 0x51e01c0,
    end_rva: 0x51e03d0,
    sha256: "2d6b20e2c40f5c9d421b5a2c96def3508dbbc732b5f10183c406debbabdec8f6",
  },
  {
    name: "CtrlSkill runtime-block and lifecycle callback registration path (script interval OnHit)",
    rva: 0x54781f0,
    end_rva: 0x5478dc0,
    sha256: "b79378f70029f9a6aa56d3dfb453499af481bb684ac561c23045d54c4e4435c6",
  },
  {
    name: "LocalAttrBattleFrameSpeed component setter path (conflicting script interval label)",
    rva: 0x5252670,
    end_rva: 0x52527c0,
    sha256: "138b2efc1c450bc1c03d272ff56e819e036df92e60882448216a5a845c787705",
  },
];

const REVIEWED_ANCHORS = [
  {
    id: "numeric-skill-event-hit-dictionary-lookup",
    rva: 0x5274bdc,
    bytes: "ba02000000498bcee8779ee8fb",
    interpretation:
      "the orchestrator requests exact ESkillEventType numeric value 2 from the parsed event dictionary",
  },
  {
    id: "numeric-skill-event-hit-calls-wrapper-with-standard-flag",
    rva: 0x5274d47,
    bytes: "4c897c24204533c94c8bc6488b542460488bcfe8c1370000",
    interpretation:
      "each numeric event-type-2 config calls the HitData wrapper with R9D=false",
  },
  {
    id: "numeric-skill-event-motion-dictionary-lookup",
    rva: 0x5274f68,
    bytes: "ba04000000498bcee8eb9ae8fb",
    interpretation:
      "the orchestrator separately requests exact ESkillEventType numeric value 4",
  },
  {
    id: "motion-config-calls-wrapper-with-common-flag",
    rva: 0x527531e,
    bytes: "4c897c242041b1014c8bc6498bd6488bcfe8ec310000",
    interpretation:
      "accepted motion configs call the HitData wrapper with R9B=true after motion-data construction",
  },
  {
    id: "stage-event-param-name-and-value-directly-enter-runtime-dictionary",
    rva: 0x52760c8,
    bytes: "41b1014c8b4720488b5710498bcde865e1eafb",
    interpretation:
      "the exact StageEventParamData.ParamName object at +0x10 is the dictionary key and ParamValue at +0x20 is its value; no localized display name or copied static candidate is substituted",
  },
  {
    id: "stage-index-and-numeric-event-type-parameters-are-extracted-from-param-values",
    rva: 0x5275ff2,
    bytes:
      "488b5810488b1573ee2d04483bda0f84810000004885db74284885d274238b4210394310751b4c6343104d03c04883c214488d4b144533c9e831f2b1fe84c07554488b15eeed2d04483bda742d4885db745d4885d274588b421039431075504c6343104d03c04883c214488d4b144533c9e8f8f1b1fe84c074354c8be748897c246833d2488b4f20e88103b0fe8bf089442434eb1a4c8bff48897c246033d2488b4f20e86603b0fe448bf089442430",
    interpretation:
      "the constructor compares each exact ParamName, parses the matching ParamValue at +0x20, and retains separate numeric stage-index and event-type values",
  },
  {
    id: "parsed-stage-index-filters-runtime-config",
    rva: 0x527611f,
    bytes: "443bb424200100000f85b60000004c8b05c4ed2d048bd64c8b742438498bcee8fd88e6fb84c0",
    interpretation:
      "the parsed stage index must equal the requested stage index before this runtime config can enter the grouped event dictionary",
  },
  {
    id: "parsed-numeric-event-type-selects-runtime-config-group",
    rva: 0x5276197,
    bytes: "4d85f60f84c50000004c8b0531ed2d048bd6498bcee8af80e1fd",
    interpretation:
      "the separately parsed numeric event type in ESI selects the outer runtime-dictionary group that receives the exact ParamName-to-ParamValue dictionary",
  },
  {
    id: "wrapper-false-selects-standard-timing-parser",
    rva: 0x527872d,
    bytes: "4584f6756ee819e0ffff84c0756e",
    interpretation:
      "the wrapper tests the common flag and, when false, calls the standard timing parser at RVA 0x5276750",
  },
  {
    id: "wrapper-true-selects-common-timing-parser",
    rva: 0x52787a0,
    bytes: "e8dbdaffff84c07492",
    interpretation:
      "the wrapper's true branch calls the common timing parser at RVA 0x5276280",
  },
  {
    id: "logic-frame-rate-initialized-to-30",
    rva: 0x3f318c2,
    bytes: "488b05cf006a05488b88b8000000c741341e000000",
    interpretation:
      "GameContext static field offset 0x34 (LogicFrameRate) is initialized to exact integer 30",
  },
  {
    id: "standard-begin-time-direct-float-parse",
    rva: 0x52769e1,
    bytes:
      "488d461444897d0b488945ff33c98b4610894507e80629a1fe0f2845ff4c8d4f244c8bc0660f7f45ffbae70000004c897c2420488d4dffe8f3cdb1fe84c0",
    interpretation:
      "the standard branch passes HitData+0x24 as the float parse destination; the digest-locked method performs no later scaling of BeginTime",
  },
  {
    id: "standard-hit-interval-divided-by-logic-frame-rate",
    rva: 0x5276ab1,
    bytes:
      "488b05e0ae3504f30f107f2c4439b8e0000000750f488bc8e8e2ba71fb488b05c3ae3504488b80b8000000488bcb660f6e40340f5bc0f30f5ef8f30f117f2c",
    interpretation:
      "HitData.HitInterval at +0x2c is divided in float32 by GameContext.LogicFrameRate at static-field offset 0x34",
  },
  {
    id: "standard-zero-count-normalization-and-end-time",
    rva: 0x5276b1a,
    bytes:
      "b80100000044397f3074038b4730894730488bcb660f6ec00f5bc0f30f59472cf30f584724f30f114728",
    interpretation:
      "DamageCount zero becomes one, then EndTime = BeginTime + float32(DamageCount) * HitInterval",
  },
  {
    id: "standard-damage-interval-divided-by-logic-frame-rate",
    rva: 0x5276b6e,
    bytes:
      "488b0523ae3504f30f107f344439b8e0000000750f488bc8e825ba71fb488b0506ae3504488b80b8000000488bcbf30f1175df660f6e40340f5bc0f30f5ef8f30f117f34",
    interpretation:
      "HitData.DamageInterval at +0x34 is divided in float32 by GameContext.LogicFrameRate at static-field offset 0x34",
  },
  {
    id: "detect-be-hit-wrapper-forwards-speed-to-scheduler-eleventh-argument",
    rva: 0x5280536,
    bytes:
      "48897c2470c64424680140887c246044887c2458f30f108424c0010000f30f11442450f30f108c24b8010000f30f114c2448f30f108424b0010000f30f11442440488d8424900000004889442438488d8424a00000004889442430895c2428488b8424a001000048894424204c8b8c2498010000458bc4498bd5488bcee8a8190000",
    interpretation:
      "the DetectBeHit wrapper reloads its incoming speed scalar and places it in stack argument 11 before directly calling the scheduler at RVA 0x5281f60",
  },
  {
    id: "detect-hit-wrapper-forwards-speed-to-scheduler-eleventh-argument",
    rva: 0x5281144,
    bytes:
      "f30f108424f8010000f30f11442450f30f108c24f0010000f30f114c2448f30f108424e8010000f30f11442440488d8424800000004889442438488d8424b80100004889442430895c24284c8bb424d00100004c897424204c8bce448bc7498bd7498bcce8b30d0000",
    interpretation:
      "the DetectHit wrapper reloads its incoming speed scalar and places it in stack argument 11 before directly calling the scheduler at RVA 0x5281f60",
  },
  {
    id: "ctrl-skill-time-factor-becomes-detect-hit-speed",
    rva: 0x547b59e,
    bytes:
      "498b46204885c00f84be000000f30f10483c33c048898424a00000004885f60f84a0000000f20f108424a0000000f20f11842480000000898424880000004c8d4b704c896c246088442458c644245001f30f114c2448f30f11742440f30f11542438488d8424800000004889442430488d84249000000048894424284c896424204533c0498bd7488bcee86358e0ff",
    interpretation:
      "this outgoing CtrlSkill path loads LogicRuntimeBlockData.TimeFactor at exact field offset 0x3c and supplies the same float as the DetectHit speed argument",
  },
  {
    id: "scheduler-divides-begin-time-by-speed",
    rva: 0x52822a2,
    bytes:
      "f30f104324450f57c9410f2fc1f3440f1095c0000000f30f10b5b0000000760ef3410f5ec20f2fc6",
    interpretation:
      "the scheduler loads its eleventh argument as speed and divides HitData.BeginTime at +0x24 by that float before the duration comparison",
  },
  {
    id: "scheduler-divides-end-time-by-speed",
    rva: 0x52822d6,
    bytes: "f30f104328410f2fc1761e33c9e8a831a501f30f104b28f30f5cf0f3410f5eca0f2ff1",
    interpretation:
      "the scheduler divides HitData.EndTime at +0x28 by the same speed scalar before the elapsed-time comparison",
  },
  {
    id: "scheduler-divides-hit-interval-by-speed",
    rva: 0x5282474,
    bytes: "f30f10532c410f28c848984883c002f3410f5ed2",
    interpretation:
      "the scheduler divides HitData.HitInterval at +0x2c by the same speed scalar",
  },
  {
    id: "scheduler-divides-damage-interval-by-speed",
    rva: 0x5282a29,
    bytes: "f30f105334410f28c848984883c002f3410f5ed2",
    interpretation:
      "the scheduler divides HitData.DamageInterval at +0x34 by the same speed scalar",
  },
  {
    id: "ctrl-skill-action-speed-helper-output-enters-component-setter",
    rva: 0x547a5af,
    bytes:
      "4533c9448bc3418bd6488bcee810a8e1ff0f28c80f57c00f2fc87708f30f100d31397d03488b4f10e8646acbfe",
    interpretation:
      "the CtrlSkill stage-exit path calls the reviewed action-speed body at RVA 0x5294dd0, substitutes exact float one only for a non-positive result, and passes the selected float to setter RVA 0x4131040",
  },
  {
    id: "component-setter-reads-and-writes-same-float-component",
    rva: 0x41310a0,
    bytes:
      "488bcfe8e80700000f28c80f28c6e8dd4d9afc84c00f85b5000000488bd7488d4c2438e8281357fc0f10000f1144242848c744243800000000488d4424284889442440488b5c24284885db0f848f000000488b0570494305488b4020f68035010000017508488bc8e853078ffc488b80c0000000488b00f68035010000017508488bc8e838078ffc4c8bc3488bd0e8cd66edfbf30f1130",
    interpretation:
      "setter RVA 0x4131040 first calls getter RVA 0x4131890 for equality short-circuiting and otherwise writes the incoming float into that same component slot",
  },
  {
    id: "component-speed-times-companion-factor-becomes-time-factor",
    rva: 0x5479ed7,
    bytes:
      "488b49100f28f20f297c2420488bfae8a5a4cdfe0f28f84885ff7450488b4b10488b7720e89079cbfe4885f6743e488b4720f30f59c7f30f11463c4885c0742cf30f59703c",
    interpretation:
      "the CtrlSkill block path reads a companion factor at RVA 0x4154390 and the same action-speed component at RVA 0x4131890, multiplies them in float32, and stores the product at LogicRuntimeBlockData.TimeFactor offset 0x3c",
  },
  {
    id: "runtime-block-constructs-stage-with-itself",
    rva: 0x51de4c9,
    bytes:
      "488b0d506f4004e8bbe284fb4533c0488bd6488bc8488bd8e81a160000833d2b9287040048895e10",
    interpretation:
      "the LogicRuntimeBlock constructor passes the new block as RDX to the LogicStage constructor and retains the resulting stage at block offset 0x10",
  },
  {
    id: "logic-stage-constructor-retains-runtime-block-argument",
    rva: 0x51dfb0f,
    bytes: "803d5c16890400488bda488bf9",
    interpretation:
      "the LogicStage constructor copies its RDX LogicRuntimeBlock argument into RBX while retaining the new stage object in RDI",
  },
  {
    id: "logic-stage-runtime-block-field-write",
    rva: 0x51dfb76,
    bytes: "48895f18",
    interpretation:
      "the LogicStage constructor stores that same LogicRuntimeBlock at stage field offset 0x18",
  },
  {
    id: "ctrl-skill-registers-stage-enter-callback",
    rva: 0x5478a36,
    bytes:
      "488b5e384885db0f8469030000488b0dbee40e04488b5b28e83d3d5bfb4c8b058e8e12044533c9488bd6488bc8488bf8e8f5fd71fb4885db0f843803000044393d9dec5d0448897b38",
    interpretation:
      "the CtrlSkill path selects its retained runtime block, selects StageData at block offset 0x28, constructs the delegate from exact metadata global RVA 0x95a18e8, and stores it at StageData.OnStageEnter offset 0x38",
  },
  {
    id: "ctrl-skill-stage-enter-method-metadata-token",
    rva: 0x95a18e8,
    bytes: "1b3b016000000000",
    interpretation:
      "the exact current-build on-disk metadata token occupies the global that the stage-enter registration loads; the address script maps this global to native body RVA 0x547a0a0",
  },
  {
    id: "logic-stage-enter-invokes-registered-callback-with-retained-block",
    rva: 0x51dfed2,
    bytes:
      "83491402488b4f204885c90f84a0020000488b49384885c97412488b41184c8b4128488b5718488b4940ffd0",
    interpretation:
      "after marking the stage active, LogicStage.Enter loads StageData.OnStageEnter at offset 0x38 and invokes it with the same LogicRuntimeBlock retained at stage offset 0x18",
  },
  {
    id: "ctrl-skill-registers-before-stage-update-callback",
    rva: 0x547873d,
    bytes:
      "488b5e384885db0f8462060000488b0dd7e70e04488b5b20e836405bfb4c8b05679112044533c9488bd6488bc8488bf8e84e96b6fc4885db0f843106000044393d96ef5d0448897b40",
    interpretation:
      "the CtrlSkill path selects BlockData at runtime-block offset 0x20, constructs the delegate from exact metadata global RVA 0x95a18c8, and stores it at BlockData.OnBeforeStageUpdate offset 0x40",
  },
  {
    id: "ctrl-skill-before-stage-update-method-metadata-token",
    rva: 0x95a18c8,
    bytes: "153b016000000000",
    interpretation:
      "the exact current-build on-disk metadata token occupies the global that before-update registration loads; the address script maps this global to native body RVA 0x5479ec0",
  },
  {
    id: "ctrl-skill-registers-stage-update-callback",
    rva: 0x5478acb,
    bytes:
      "488b5e384885db0f84d4020000488b0d49e40e04488b5b28e8a83c5bfb4c8b05518e12044533c9488bd6488bc8488bf8e8c092b6fc4885db0f84a302000044393d08ec5d0448897b40",
    interpretation:
      "the CtrlSkill path selects StageData at runtime-block offset 0x28, constructs the delegate from exact metadata global RVA 0x95a1940, and stores it at StageData.OnStageUpdate offset 0x40",
  },
  {
    id: "ctrl-skill-stage-update-method-metadata-token",
    rva: 0x95a1940,
    bytes: "1d3b016000000000",
    interpretation:
      "the exact current-build on-disk metadata token occupies the global that stage-update registration loads; the address script maps this global to native body RVA 0x547afa0",
  },
  {
    id: "runtime-block-dispatches-before-stage-and-after-in-order",
    rva: 0x51de78f,
    bytes:
      "488b4b204885c90f843d020000488b49404885c97414488b41180f28d64c8b4928488bd3488b4940ffd0488b43204885c00f8413020000488b4b104885c90f8406020000f30f10503c4533c90f28cee8dd190000488b4b204885c90f84e9010000488b49484885c97414488b41180f28d64c8b4928488bd3488b4940ffd0",
    interpretation:
      "one runtime update invokes BlockData.OnBeforeStageUpdate with the current block and dt, calls the retained stage with dt and that block's TimeFactor, then invokes BlockData.OnAfterStageUpdate with the same block and dt",
  },
  {
    id: "logic-stage-update-invokes-stage-callback-with-retained-block",
    rva: 0x51e0375,
    bytes:
      "488b4b204885c97431488b49404885c97415488b41184c8b49280f28d6488b5318488b4940ffd0",
    interpretation:
      "after its time-factor-scaled update list, LogicStage.Update invokes StageData.OnStageUpdate at offset 0x40 with the same retained LogicRuntimeBlock and unmodified dt",
  },
  {
    id: "companion-getter-selects-battle-frame-component-metadata",
    rva: 0x41543a2,
    bytes:
      "488d0d279e4105e8123e89fcf0830c2400488d0d269e4105e8013e89fcf0830c2400",
    interpretation:
      "the companion getter initializes exact metadata globals 0x956e1d0 and 0x956e1e0, which the address script binds to LocalAttrBattleFrameSpeedComponent generic operations",
  },
  {
    id: "companion-getter-reads-battle-frame-float",
    rva: 0x41543cb,
    bytes:
      "4885db7462488b4b204885c97459e8f21155fc488bd84885c0744c488b05f39d4105488b4020f68035010000017508488bc8e85ed48cfc488b80c0000000488b00f68035010000017508488bc8e843d48cfc4c8bc3488bd0e8d833ebfbf30f1000",
    interpretation:
      "the getter selects the entity's pure-component storage, resolves the exact LocalAttrBattleFrameSpeed component accessor, and returns its float at component offset zero",
  },
  {
    id: "battle-frame-component-metadata-tokens",
    rva: 0x956e1d0,
    bytes: "9d370cc000000000477e0120000000003b1b0ac000000000",
    interpretation:
      "the digest-locked metadata globals used by the companion getter retain their exact current-build on-disk tokens",
  },
  {
    id: "battle-frame-setter-writes-incoming-float",
    rva: 0x52526d0,
    bytes:
      "488bcfe8b81cf0fe0f28c80f28c6e8ad3788fb84c00f85b5000000488bd7488d4c2438e8b8a45ffb0f10000f1144242848c744243800000000488d4424284889442440488b5c24284885db0f848f000000488b0548a33a04488b4020f68035010000017508488bc8e823f17cfb488b80c0000000488b00f68035010000017508488bc8e808f17cfb4c8bc3488bd0e89d50dbfaf30f1130",
    interpretation:
      "the component-specific setter compares the incoming float with getter RVA 0x4154390, obtains the LocalAttrBattleFrameSpeed writer when changed, and stores the incoming float at component offset zero",
  },
  {
    id: "local-player-controller-initializes-battle-frame-speed-to-one",
    rva: 0x545712e,
    bytes: "33d2488bcfe8982a0000f30f100dc46d7f03488b4f10e827b5dfff",
    interpretation:
      "the exact PlayerCtrlComp update path loads float32 one from RVA 0x8c4df04 and passes it to the LocalAttrBattleFrameSpeed setter at RVA 0x5252670",
  },
  {
    id: "exact-float32-one-constant",
    rva: 0x8c4df04,
    bytes: "0000803f",
    interpretation: "the selected current-build constant is exact IEEE-754 binary32 1.0",
  },
];

const DUMP_EVIDENCE = [
  "public class HitData : IObjPooled",
  "public float BeginTime; // 0x24",
  "public float EndTime; // 0x28",
  "public float HitInterval; // 0x2C",
  "public int DamageCount; // 0x30",
  "public float DamageInterval; // 0x34",
  "public float TimeFactor; // 0x3C",
  "public struct LocalAttrBattleFrameSpeedComponent",
  "public float Value; // 0x0",
  "private readonly LogicStage stage_; // 0x10",
  "public readonly LogicRuntimeBlockData BlockData; // 0x20",
  "public readonly LogicStageRuntimeData StageData; // 0x28",
  "public Action<LogicRuntimeBlock, float> OnBeforeStageUpdate; // 0x40",
  "public Action<LogicRuntimeBlock, float> OnAfterStageUpdate; // 0x48",
  "private readonly LogicRuntimeBlock rtBlock_; // 0x18",
  "public static int LogicFrameRate; // 0x34",
  "// RVA: 0x5276750 Offset: 0x5275150 VA: 0x185276750",
  "private void parseHit(ZDictionary<string, string> hitConfig, long skillEffectId, bool forOffset = False)",
  "// RVA: 0x3F31740 Offset: 0x3F30140 VA: 0x183F31740",
  "private static void .cctor()",
  "public const ESkillEventType SkillEventHit = 2;",
  "public const ESkillEventType SkillEventMotion = 4;",
  "float speed = 1",
];

const SCRIPT_EVIDENCE = [
  `\"Address\": ${GAME_CONTEXT_TYPE_INFO_RVA}`,
  '"Name": "Panda.Core.GameContext_TypeInfo"',
  '"Signature": "Panda_Core_GameContext_c*"',
  '"Address": 156585512',
  '"Value": "drJHMMdWDOUuXQD"',
  '"Address": 156899528',
  '"Name": "Method$Panda.ZGame.CtrlSkillComp.beforeLogicBlockRecycle()"',
  '"MethodAddress": 88579776',
  '"Address": 156899560',
  '"Name": "Method$Panda.ZGame.CtrlSkillComp.OnStageExit()"',
  '"MethodAddress": 88580256',
  '"Address": 156899648',
  '"Name": "Method$Panda.ZGame.CtrlSkillComp.syncSkillStageEnd()"',
  '"MethodAddress": 88584096',
  '"Address": 156688848',
  '"Name": "Method$Panda.ZGame.Pure.ZPureComponentStorage.Write\\u003CLocalAttrBattleFrameSpeedComponent\\u003E()"',
  '"Address": 156688864',
  '"Name": "Method$Panda.ZGame.Pure.PureComponentChangedEvent\\u003CLocalAttrBattleFrameSpeedComponent\\u003E.get_Aspect()"',
  '"Address": 88435440',
  '"Name": "Panda.ZGame.PlayerCtrlComp$$OnUpdate"',
];

function fail(message) {
  throw new Error(message);
}

function take(values, flag) {
  const index = values.indexOf(flag);
  if (index < 0 || index + 1 >= values.length) fail(`${flag} requires a value`);
  const value = values[index + 1];
  values.splice(index, 2);
  return value;
}

function parseArguments(argv) {
  const values = [...argv];
  const command = values.shift();
  if (command === "verify") {
    const input = path.resolve(take(values, "--input"));
    if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
    return { command, input };
  }
  if (command !== "generate") fail("expected generate or verify");
  const result = {
    command,
    build: take(values, "--build"),
    binary: path.resolve(take(values, "--binary")),
    identity: path.resolve(take(values, "--identity")),
    dump: path.resolve(take(values, "--dump")),
    script: path.resolve(take(values, "--script")),
    battleFrameReferenceAudit: path.resolve(
      take(values, "--battle-frame-reference-audit"),
    ),
    battleFrameSetterCallsiteAudit: path.resolve(
      take(values, "--battle-frame-setter-callsite-audit"),
    ),
    output: path.resolve(take(values, "--output")),
  };
  if (values.length) fail(`unexpected arguments: ${values.join(" ")}`);
  return result;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return sha256(Buffer.from(JSON.stringify(copy)));
}

function float32Evidence(value) {
  const normalized = Math.fround(value);
  const bytes = Buffer.alloc(4);
  bytes.writeFloatLE(normalized);
  return { decimal: normalized, little_endian_bits_hex: bytes.toString("hex") };
}

function float32CancellationCounterexample() {
  const actionSpeed = Math.fround(1.2);
  const counterfactualActionSpeed = Math.fround(1.1);
  const battleFrameSpeed = Math.fround(0.9);
  const actualComposed = Math.fround(actionSpeed * battleFrameSpeed);
  const counterfactualComposed = Math.fround(
    counterfactualActionSpeed * battleFrameSpeed,
  );
  const composedRatio = actualComposed / counterfactualComposed;
  const cancelledRatio = actionSpeed / counterfactualActionSpeed;
  if (composedRatio === cancelledRatio) fail("float32 cancellation counterexample collapsed");
  return {
    action_speed: float32Evidence(actionSpeed),
    counterfactual_action_speed: float32Evidence(counterfactualActionSpeed),
    battle_frame_speed: float32Evidence(battleFrameSpeed),
    actual_composed_speed: float32Evidence(actualComposed),
    counterfactual_composed_speed: float32Evidence(counterfactualComposed),
    composed_speed_ratio: composedRatio,
    algebraically_cancelled_action_speed_ratio: cancelledRatio,
    exactly_equal: false,
  };
}

function validateBattleFrameAudits(referenceAudit, setterAudit) {
  const expectedReferencePairs = [
    [0x6a55dd, 0x956e1d0],
    [0x84cbbf, 0x95fcae8],
    [0x84d459, 0x95fcb00],
    [0x85aca9, 0x95fcb00],
    [0x41543a2, 0x956e1d0],
    [0x41543b3, 0x956e1e0],
    [0x41543e6, 0x956e1e0],
    [0x525268e, 0x95fcae8],
    [0x525269f, 0x95fcb00],
    [0x52526b0, 0x95fca70],
    [0x5252721, 0x95fca70],
    [0x5252772, 0x95fcb00],
    [0x5252785, 0x95fcb00],
    [0x53c4992, 0x956e1d0],
    [0x53c49a3, 0x956e1e0],
    [0x53c49d6, 0x956e1e0],
    [0x53c4a4e, 0x95fcae8],
    [0x53c4a5f, 0x95fcb00],
    [0x53c4a70, 0x95fca70],
    [0x53c4ae1, 0x95fca70],
    [0x53c4b32, 0x95fcb00],
    [0x53c4b45, 0x95fcb00],
  ];
  const observedReferencePairs = (referenceAudit.references ?? []).map((row) => [
    Number(row.instruction_rva),
    Number(row.effective_target_rva),
  ]);
  const expectedSetterCalls = [0x523f17a, 0x523f3e8, 0x5457144, 0x548ed66];
  const observedSetterCalls = (setterAudit.callsites ?? []).map((row) =>
    Number(row.call_rva),
  );
  const computedSetterCallsite = (setterAudit.callsites ?? []).find(
    (row) => Number(row.call_rva) === 0x523f3e8,
  );
  const computedSetterInstructions = new Map(
    (computedSetterCallsite?.disassembly ?? []).map((row) => [
      Number(row.rva),
      `${row.mnemonic} ${row.operands}`,
    ]),
  );
  const expectedComputedSetterInstructions = new Map([
    [0x523f36a, "divss xmm6, dword ptr [rcx + 0x14]"],
    [0x523f373, "movss xmm0, dword ptr [rcx + 0x10]"],
    [0x523f378, "minss xmm6, xmm1"],
    [0x523f37c, "subss xmm0, xmm1"],
    [0x523f380, "mulss xmm6, xmm0"],
    [0x523f384, "addss xmm6, xmm1"],
    [0x523f3b2, "movaps xmm1, xmm6"],
    [0x523f3e8, "call 0x5252670"],
  ]);
  if (
    Number(referenceAudit?.schema_version) !== 1 ||
    referenceAudit?.game_build !== EXPECTED_BUILD ||
    referenceAudit?.binary?.sha256 !== EXPECTED_BINARY_SHA256 ||
    Number(referenceAudit?.summary?.exact_rip_relative_references) !== 22 ||
    Number(referenceAudit?.summary?.target_rvas_with_references) !== 5 ||
    referenceAudit?.resource_bounds?.one_decoder_chunk_buffered_at_a_time !== true ||
    Number(referenceAudit?.resource_bounds?.maximum_decoder_buffer_bytes) > 1_048_591 ||
    Number(referenceAudit?.resource_bounds?.measured_process_peak_working_set_bytes) >
      512 * 1024 * 1024 ||
    referenceAudit?.policy?.provider_rdps_credit_allowed !== false ||
    JSON.stringify(observedReferencePairs) !== JSON.stringify(expectedReferencePairs) ||
    Number(setterAudit?.schema_version) !== 3 ||
    setterAudit?.game_build !== EXPECTED_BUILD ||
    setterAudit?.binary?.sha256 !== EXPECTED_BINARY_SHA256 ||
    Number(setterAudit?.summary?.selected_exact_target_rvas) !== 2 ||
    Number(setterAudit?.summary?.direct_callsites) !== 4 ||
    JSON.stringify(observedSetterCalls) !== JSON.stringify(expectedSetterCalls) ||
    (setterAudit.callsites ?? []).some((row) => Number(row.target_rva) !== 0x5252670) ||
    Number(computedSetterCallsite?.caller?.start_rva) !== 0x523f190 ||
    !computedSetterCallsite?.caller?.names?.includes("Panda.ZGame.ZFreeModelShowInfo$$Init") ||
    [...expectedComputedSetterInstructions].some(
      ([rva, instruction]) => computedSetterInstructions.get(rva) !== instruction,
    )
  ) {
    fail("battle-frame component reference or setter-callsite audit changed");
  }
  return {
    exact_component_metadata_rip_references: observedReferencePairs.length,
    component_specific_setter_direct_callsites: observedSetterCalls.length,
    computed_float32_setter_path_callsite_rva: 0x523f3e8,
    computed_float32_setter_path_address_interval_name:
      "Panda.ZGame.ZFreeModelShowInfo$$Init",
    address_interval_name_is_behavioral_or_entity_domain_authority: false,
    affected_host_is_preview_only_proven: false,
    globally_constant_float32_one_proven: false,
    unused_duplicate_setter_direct_callsites: 0,
    bounded_reference_audit_peak_working_set_bytes: Number(
      referenceAudit.resource_bounds.measured_process_peak_working_set_bytes,
    ),
  };
}

async function streamEvidence(file, requiredStrings = []) {
  const hash = createHash("sha256");
  const found = new Set();
  const maxNeedle = Math.max(1, ...requiredStrings.map((value) => Buffer.byteLength(value)));
  let tail = Buffer.alloc(0);
  let bytes = 0;
  for await (const chunk of createReadStream(file, { highWaterMark: 1024 * 1024 })) {
    hash.update(chunk);
    bytes += chunk.length;
    const combined = tail.length ? Buffer.concat([tail, chunk]) : chunk;
    const text = combined.toString("utf8");
    for (const required of requiredStrings) {
      if (!found.has(required) && text.includes(required)) found.add(required);
    }
    tail = combined.subarray(Math.max(0, combined.length - maxNeedle + 1));
  }
  const missing = requiredStrings.filter((required) => !found.has(required));
  if (missing.length) fail(`${file} is missing reviewed evidence: ${missing.join(", ")}`);
  return { file, bytes, sha256: hash.digest("hex") };
}

function readRange(file, offset, length) {
  const descriptor = openSync(file, "r");
  try {
    const bytes = Buffer.alloc(length);
    const observed = readSync(descriptor, bytes, 0, length, offset);
    if (observed !== length) fail(`short read at file offset ${offset}`);
    return bytes;
  } finally {
    closeSync(descriptor);
  }
}

function parsePe(file) {
  const header = readRange(file, 0, 64 * 1024);
  if (header.toString("ascii", 0, 2) !== "MZ") fail("GameAssembly is not a PE image");
  const pe = header.readUInt32LE(0x3c);
  if (header.toString("ascii", pe, pe + 4) !== "PE\0\0") fail("missing PE signature");
  const sectionCount = header.readUInt16LE(pe + 6);
  const optionalHeaderBytes = header.readUInt16LE(pe + 20);
  const sectionTable = pe + 24 + optionalHeaderBytes;
  const sections = [];
  for (let index = 0; index < sectionCount; index += 1) {
    const offset = sectionTable + index * 40;
    sections.push({
      name: header.toString("ascii", offset, offset + 8).replaceAll("\0", ""),
      virtual_size: header.readUInt32LE(offset + 8),
      virtual_address: header.readUInt32LE(offset + 12),
      raw_size: header.readUInt32LE(offset + 16),
      raw_pointer: header.readUInt32LE(offset + 20),
    });
  }
  return sections;
}

function offsetForRva(sections, rva) {
  const section = sections.find(
    (candidate) =>
      rva >= candidate.virtual_address &&
      rva < candidate.virtual_address + Math.max(candidate.virtual_size, candidate.raw_size),
  );
  if (!section) fail(`RVA 0x${rva.toString(16)} is outside the PE sections`);
  return section.raw_pointer + rva - section.virtual_address;
}

function bytesAt(file, sections, rva, length) {
  return readRange(file, offsetForRva(sections, rva), length);
}

function validateReport(report) {
  if (
    Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.game_build !== EXPECTED_BUILD ||
    report?.summary?.exact_current_build_binary_identity !== true ||
    report?.summary?.standard_hitdata_native_timing_formula_proven !== true ||
    report?.summary?.standard_hit_event_to_parser_route_proven !== true ||
    report?.summary?.stage_event_parameter_name_to_runtime_dictionary_key_proven !== true ||
    report?.summary?.parser_lookup_global_to_catalog_parameter_identity_proven !== false ||
    report?.summary?.standard_parser_catalog_parameter_mapping_proven !== false ||
    report?.summary?.common_parser_catalog_parameter_mapping_proven !== false ||
    report?.summary?.wrapper_speed_to_scheduler_parameter_proven !== true ||
    report?.summary?.time_factor_to_outgoing_hit_scheduler_proven !== true ||
    report?.summary?.scheduler_speed_scaling_formula_proven !== true ||
    report?.summary?.action_speed_formula_to_scheduler_mechanism_route_proven !== true ||
    report?.summary?.ctrl_skill_callback_registration_and_dispatch_order_proven !== true ||
    report?.summary?.action_speed_formula_native_sampling_point_proven !== true ||
    report?.summary?.companion_factor_component_identity_proven !== true ||
    report?.summary?.battle_frame_component_reference_surface_proven !== true ||
    report?.summary?.computed_battle_frame_setter_path_proven !== true ||
    report?.summary?.battle_frame_globally_constant_proven !== false ||
    report?.summary?.computed_setter_host_preview_only_proven !== false ||
    report?.summary?.exact_float32_battle_frame_cancellation_authorized !== false ||
    report?.summary?.action_speed_formula_to_each_scheduler_invocation_proven !== false ||
    report?.summary?.motion_curve_event_to_parser_route_proven !== false ||
    report?.summary?.action_start_to_damage_packet_clock_join_proven !== false ||
    report?.summary?.provider_rdps_credit_allowed !== false ||
    report?.summary?.ui_rdps_display_allowed !== false ||
    Number(report?.summary?.observed_damage_reassigned_to_provider) !== 0 ||
    report?.content_sha256 !== contentHash(report)
  ) {
    fail("native damage-event timing proof is inconsistent or unsafe");
  }
}

async function generate(options) {
  if (options.build !== EXPECTED_BUILD) fail(`this proof supports build ${EXPECTED_BUILD}`);
  if (existsSync(options.output)) fail(`refusing to overwrite ${options.output}`);
  const identityBytes = readFileSync(options.identity);
  const identity = JSON.parse(identityBytes);
  if (
    identity.deployment !== EXPECTED_DEPLOYMENT ||
    String(identity.game_build) !== options.build ||
    Number(identity.game_assembly?.byte_length) !== EXPECTED_BINARY_BYTES ||
    String(identity.game_assembly?.sha256) !== EXPECTED_BINARY_SHA256
  ) {
    fail("client identity is not the reviewed exact current build");
  }
  const battleFrameReferenceAuditBytes = readFileSync(options.battleFrameReferenceAudit);
  const battleFrameSetterCallsiteAuditBytes = readFileSync(
    options.battleFrameSetterCallsiteAudit,
  );
  if (
    sha256(battleFrameReferenceAuditBytes) !== EXPECTED_BATTLE_FRAME_REFERENCE_AUDIT_SHA256 ||
    sha256(battleFrameSetterCallsiteAuditBytes) !==
      EXPECTED_BATTLE_FRAME_SETTER_CALLSITE_AUDIT_SHA256
  ) {
    fail("battle-frame audit artifact identity changed");
  }
  const battleFrameAuditSummary = validateBattleFrameAudits(
    JSON.parse(battleFrameReferenceAuditBytes),
    JSON.parse(battleFrameSetterCallsiteAuditBytes),
  );

  const binaryReceipt = await streamEvidence(options.binary);
  if (
    binaryReceipt.bytes !== EXPECTED_BINARY_BYTES ||
    binaryReceipt.sha256 !== EXPECTED_BINARY_SHA256
  ) {
    fail("GameAssembly bytes do not match the reviewed exact current build");
  }
  const dumpReceipt = await streamEvidence(options.dump, DUMP_EVIDENCE);
  const scriptReceipt = await streamEvidence(options.script, SCRIPT_EVIDENCE);
  const sections = parsePe(options.binary);

  const methodRegions = METHOD_REGIONS.map((method) => {
    const bytes = bytesAt(options.binary, sections, method.rva, method.end_rva - method.rva);
    if (sha256(bytes) !== method.sha256) fail(`${method.name} method-region digest changed`);
    return { ...method, bytes: bytes.length };
  });
  const anchors = REVIEWED_ANCHORS.map((anchor) => {
    const expected = Buffer.from(anchor.bytes, "hex");
    const observed = bytesAt(options.binary, sections, anchor.rva, expected.length);
    if (!observed.equals(expected)) fail(`${anchor.id} instruction bytes changed`);
    return { ...anchor, instruction_bytes_sha256: sha256(observed) };
  });

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-action-damage-event-native-timing-proof.mjs",
    game: "blue-protocol-star-resonance",
    deployment: EXPECTED_DEPLOYMENT,
    game_build: options.build,
    proof_state:
      "native-parser-and-scheduler-field-formulas-proven-catalog-key-observed-speed-and-packet-clock-joins-open",
    inputs: {
      client_binary_identity: {
        file: options.identity,
        bytes: identityBytes.length,
        sha256: sha256(identityBytes),
      },
      game_assembly: binaryReceipt,
      il2cpp_dump: dumpReceipt,
      il2cpp_address_script: scriptReceipt,
      battle_frame_component_reference_audit: {
        file: options.battleFrameReferenceAudit,
        bytes: battleFrameReferenceAuditBytes.length,
        sha256: sha256(battleFrameReferenceAuditBytes),
      },
      battle_frame_setter_callsite_audit: {
        file: options.battleFrameSetterCallsiteAudit,
        bytes: battleFrameSetterCallsiteAuditBytes.length,
        sha256: sha256(battleFrameSetterCallsiteAuditBytes),
      },
    },
    policy: {
      executes_game_code: false,
      exact_numeric_ids_offsets_and_build_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      native_method_regions_are_digest_locked: true,
      remote_player_cast_packets_required: false,
      missing_remote_casts_are_synthesized: false,
      current_character_snapshot_backfill_allowed: false,
      ordinary_damage_totals_unchanged: true,
      algebraic_real_number_cancellation_is_float32_authority: false,
      current_local_battle_frame_value_backfills_remote_actions: false,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
    },
    native_identity: {
      game_context_type_info_global_rva: GAME_CONTEXT_TYPE_INFO_RVA,
      game_context_type_info_script_name: "Panda.Core.GameContext_TypeInfo",
      logic_frame_rate_static_field_offset: 0x34,
      logic_frame_rate_value: LOGIC_FRAME_RATE,
      hit_data_field_offsets: {
        begin_time: 0x24,
        end_time: 0x28,
        hit_interval: 0x2c,
        damage_count: 0x30,
        damage_interval: 0x34,
      },
      ctrl_skill_callback_metadata_mappings: [
        {
          metadata_global_rva: 0x95a18e8,
          on_disk_token_hex: "1b3b016000000000",
          script_method_address_rva: 0x547a0a0,
          registered_field: "LogicStageRuntimeData.OnStageEnter",
          registered_field_offset: 0x38,
          behavioral_identity: "action-speed formula sampling and component setter",
        },
        {
          metadata_global_rva: 0x95a18c8,
          on_disk_token_hex: "153b016000000000",
          script_method_address_rva: 0x5479ec0,
          registered_field: "LogicRuntimeBlockData.OnBeforeStageUpdate",
          registered_field_offset: 0x40,
          behavioral_identity: "component-factor composition into TimeFactor",
        },
        {
          metadata_global_rva: 0x95a1940,
          on_disk_token_hex: "1d3b016000000000",
          script_method_address_rva: 0x547afa0,
          registered_field: "LogicStageRuntimeData.OnStageUpdate",
          registered_field_offset: 0x40,
          behavioral_identity: "outgoing hit update and TimeFactor forwarding",
        },
      ],
      companion_factor_component: {
        exact_type: "LocalAttrBattleFrameSpeedComponent",
        value_field_offset: 0,
        getter_rva: 0x4154390,
        component_specific_setter_rva: 0x5252670,
        local_player_controller_initializes_exact_float32_one: true,
        initialization_callsite_rva: 0x5457144,
        constant_rva: 0x8c4df04,
        constant_ieee754_little_endian_hex: "0000803f",
        exact_value_for_every_observed_damage_action_proven: false,
        computed_float32_setter_path_proven: true,
        globally_constant_float32_one_proven: false,
        computed_setter_host_preview_only_proven: false,
        current_local_player_value_may_backfill_remote_or_historical_actions: false,
        exact_reference_and_direct_setter_call_surface: battleFrameAuditSummary,
      },
      script_method_names_are_behavioral_identity_authority: false,
    },
    numeric_event_route: {
      parameter_name: "ESkillEventType",
      exact_current_build_script_literal: {
        address: 156585512,
        encoded_value: "drJHMMdWDOUuXQD",
        reviewed_byte_transform: "each UTF-8 byte XOR 0x21",
        decoded_value: "ESkillEventType",
      },
      skill_event_hit: {
        numeric_value: 2,
        exact_enum_member: "SkillEventHit",
        wrapper_common_flag: false,
        selected_parser_rva: 0x5276750,
        route_proven: true,
      },
      skill_event_motion: {
        numeric_value: 4,
        exact_enum_member: "SkillEventMotion",
        accepted_motion_wrapper_common_flag: true,
        selected_parser_rva: 0x5276280,
        motion_subtype_parameter_mapping_proven: false,
      },
      localized_or_display_event_names_are_route_authority: false,
    },
    runtime_event_dictionary: {
      exact_current_build_constructor_rva: 0x5275ae0,
      stage_event_param_data_layout: {
        param_name_offset: 0x10,
        param_type_offset: 0x18,
        param_value_offset: 0x20,
      },
      key_source: "StageEventParamData.ParamName object at exact offset 0x10",
      value_source: "StageEventParamData.ParamValue object at exact offset 0x20",
      param_name_to_dictionary_key_copied_directly: true,
      param_value_to_dictionary_value_copied_directly: true,
      stage_index_filters_requested_stage: true,
      numeric_event_type_is_outer_dictionary_key: true,
      raw_protected_string_literal_labels_are_semantic_authority_without_dataflow: false,
      parser_lookup_global_to_catalog_parameter_identity_proven: false,
    },
    reviewed_method_regions: methodRegions,
    reviewed_instruction_anchors: anchors,
    standard_hitdata_timing: {
      float_operation_width: "IEEE-754 binary32",
      begin_time_seconds: "parse_float(native_lookup_key_for_HitData_offset_0x24)",
      hit_interval_seconds:
        `parse_float(native_lookup_key_for_HitData_offset_0x2c) / ${LOGIC_FRAME_RATE}`,
      effective_damage_count:
        "parsed_int_at_HitData_offset_0x30 == 0 ? 1 : parsed_int_at_HitData_offset_0x30",
      end_time_seconds:
        "float32(begin_time_seconds + float32(effective_damage_count) * hit_interval_seconds)",
      damage_interval_seconds:
        `parse_float(native_lookup_key_for_HitData_offset_0x34) / ${LOGIC_FRAME_RATE}`,
      end_time_is_last_damage_occurrence_time: false,
      end_time_is_reviewed_parser_window_terminal: true,
      numeric_event_type_2_to_this_native_branch_proven: true,
      damage_packet_timestamp_to_native_event_clock_proven: false,
      catalog_parameter_name_to_native_lookup_key_mapping_proven: false,
      offline_catalog_values_may_materialize_this_formula: false,
    },
    live_scheduler_speed_scaling: {
      native_scheduler_rva: 0x5281f60,
      speed_parameter_ordinal: 11,
      float_operation_width: "IEEE-754 binary32",
      wrapper_routes: [
        {
          wrapper_rva: 0x52801d0,
          public_abi_evidence: "DetectBeHit(..., float duration, float hitScale, float speed)",
          speed_forwarded_unchanged_to_scheduler: true,
        },
        {
          wrapper_rva: 0x5280e90,
          public_abi_evidence:
            "DetectHit(..., float duration, float hitScale, float speed, bool isPlayerHit, bool isPassiveSkill)",
          speed_forwarded_unchanged_to_scheduler: true,
        },
      ],
      exact_effective_timing: {
        begin_time_seconds: "float32(parser_begin_time_seconds / speed)",
        end_time_seconds: "float32(parser_end_time_seconds / speed)",
        hit_interval_seconds: "float32(parser_hit_interval_seconds / speed)",
        damage_interval_seconds: "float32(parser_damage_interval_seconds / speed)",
      },
      ctrl_skill_outgoing_hit_path: {
        logic_runtime_block_data_field: "TimeFactor",
        exact_field_offset: 0x3c,
        supplied_as_detect_hit_speed: true,
        supplied_speed_then_forwarded_to_scheduler: true,
      },
      ctrl_skill_action_speed_mechanism_route: {
        reviewed_action_speed_body_rva: 0x5294dd0,
        action_speed_component_setter_rva: 0x4131040,
        action_speed_component_getter_rva: 0x4131890,
        companion_speed_factor_getter_rva: 0x4154390,
        companion_speed_factor_component: "LocalAttrBattleFrameSpeedComponent.Value",
        logic_runtime_block_data_time_factor_offset: 0x3c,
        exact_float32_composition:
          "TimeFactor = float32(action_speed_formula_factor * companion_speed_factor)",
        scheduler_speed:
          "the outgoing CtrlSkill path supplies LogicRuntimeBlockData.TimeFactor unchanged",
        mechanism_route_proven: true,
        exact_native_lifecycle: {
          action_speed_formula_sampling_point: "registered StageData.OnStageEnter callback",
          action_speed_formula_stage_enter_body_rva: 0x547a0a0,
          time_factor_composition_point:
            "registered BlockData.OnBeforeStageUpdate callback before every stage update",
          time_factor_composition_body_rva: 0x5479ec0,
          outgoing_hit_update_point: "registered StageData.OnStageUpdate callback",
          outgoing_hit_stage_update_body_rva: 0x547afa0,
          same_logic_runtime_block_for_callbacks: true,
          before_update_precedes_stage_update_in_same_dispatch: true,
          native_callback_registration_and_dispatch_order_proven: true,
          native_formula_sampling_point_proven: true,
        },
        callback_instance_join_for_each_observed_damage_action_proven: false,
        companion_factor_value_for_each_observed_damage_action_proven: false,
        battle_frame_counterfactual_cancellation: {
          real_number_symbolic_identity:
            "(action_speed * battle_frame) / (counterfactual_action_speed * battle_frame) = action_speed / counterfactual_action_speed",
          exact_float32_composition_cancels_without_battle_frame_value: false,
          reviewed_counterexample: float32CancellationCounterexample(),
          exact_battle_frame_value_or_exact_composed_speed_required: true,
          provider_removed_timing_ratio_materialized: false,
        },
      },
      zero_or_negative_speed_semantics_proven: false,
      applies_to_every_damage_action: false,
      exact_action_speed_formula_output_to_each_scheduler_invocation_proven: false,
      exact_temporary_attribute_sampling_point_proven: true,
    },
    offset_or_common_timing: {
      separate_native_parser_present: true,
      motion_curve_event_to_parser_route_proven: false,
      digest_locked_field_operations: {
        hit_data_begin_time_offset_0x24: `parse_float(native_lookup_key_a) / ${LOGIC_FRAME_RATE}`,
        hit_data_end_time_offset_0x28: `parse_float(native_lookup_key_b) / ${LOGIC_FRAME_RATE}`,
        hit_data_damage_interval_offset_0x34:
          `parse_float(native_lookup_key_c) / ${LOGIC_FRAME_RATE}`,
        hit_data_max_hit_count_offset_0x98:
          "parse_positive_int(optional_native_lookup_key_d) or native default",
      },
      catalog_parameter_name_to_native_lookup_key_mapping_proven: false,
      offline_formula_authority: false,
    },
    summary: {
      exact_current_build_binary_identity: true,
      exact_current_build_method_identity: true,
      exact_logic_frame_rate_value_proven: true,
      standard_hitdata_native_timing_formula_proven: true,
      standard_hit_event_to_parser_route_proven: true,
      stage_event_parameter_name_to_runtime_dictionary_key_proven: true,
      parser_lookup_global_to_catalog_parameter_identity_proven: false,
      standard_parser_catalog_parameter_mapping_proven: false,
      common_parser_catalog_parameter_mapping_proven: false,
      wrapper_speed_to_scheduler_parameter_proven: true,
      time_factor_to_outgoing_hit_scheduler_proven: true,
      scheduler_speed_scaling_formula_proven: true,
      action_speed_formula_to_scheduler_mechanism_route_proven: true,
      ctrl_skill_callback_registration_and_dispatch_order_proven: true,
      action_speed_formula_native_sampling_point_proven: true,
      companion_factor_component_identity_proven: true,
      battle_frame_component_reference_surface_proven: true,
      computed_battle_frame_setter_path_proven: true,
      battle_frame_globally_constant_proven: false,
      computed_setter_host_preview_only_proven: false,
      exact_float32_battle_frame_cancellation_authorized: false,
      action_speed_formula_to_each_scheduler_invocation_proven: false,
      motion_curve_event_to_parser_route_proven: false,
      action_start_to_damage_packet_clock_join_proven: false,
      transport_ordering_and_latency_bound_proven: false,
      provider_removed_action_opportunity_proven: false,
      provider_rdps_credit_allowed: false,
      ui_rdps_display_allowed: false,
      runtime_promotion_allowed: false,
      observed_damage_reassigned_to_provider: 0,
    },
    blockers: [
      "the exact CtrlSkill callback lifecycle and LocalAttrBattleFrameSpeed write surface are proven, including a computed float32 setter path whose affected host is not proven preview-only; float32 composition prevents exact algebraic cancellation without the observed companion or composed scheduler-speed value",
      "zero or negative scheduler speed behavior is not authorized for offline replay",
      "the exact native lookup keys have not been joined to the current-build catalog parameter identities, so neither standard nor common parser arithmetic may be materialized from similarly named catalog fields",
      "motion subtype parameters are not yet mapped to the common parser's exact timing keys and units",
      "HitData EndTime is a parser window terminal, not yet proven to be the last damage occurrence",
      "the damage packet timestamp/sequence clock has not been joined to the native stage-event clock with a transport latency bound",
      "provider-removed opportunity, integer rounding, and conservation replay remain unproven",
      "current-build protocol-pack identity and required replay gates remain missing",
    ],
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(options.output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    `proved exact build ${options.build} native HitData field arithmetic; catalog-key mapping=false; provider credit=false\nwrote ${options.output}\n`,
  );
}

const options = parseArguments(process.argv.slice(2));
if (options.command === "generate") await generate(options);
else {
  const report = JSON.parse(readFileSync(options.input));
  validateReport(report);
  process.stdout.write(
    `verified build ${report.game_build} native HitData field arithmetic; catalog-key mapping=false; provider credit=false\n`,
  );
}
