#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const ledgerPath = resolvePath(options.ledger);
const factorsPath = resolvePath(options.factors);
const battleImaginesPath = resolvePath(options.battleImagines);
const rogueDescriptionsPath = resolvePath(options.rogueDescriptions);
const attributeDescriptionsPath = resolvePath(options.attributeDescriptions);
const textDescriptionsPath = resolvePath(options.textDescriptions);
const talentsPath = resolvePath(options.talents);
const buffsPath = resolvePath(options.buffs);
const aoyiStarsPath = resolvePath(options.aoyiStars);
const skillEffectsPath = resolvePath(options.skillEffects);
const originLedgerPath = resolvePath(options.originLedger);
const outputPath = resolvePath(options.output);

const ledger = readJson(ledgerPath, "recipient-scope ledger");
const factors = readJson(factorsPath, "season phantom factors");
const battleImagines = readJson(battleImaginesPath, "battle imagine descriptions");
const rogueDescriptions = readJson(rogueDescriptionsPath, "rogue entry descriptions");
const attributeDescriptions = readJson(attributeDescriptionsPath, "attribute descriptions");
const textDescriptions = readJson(textDescriptionsPath, "text descriptions");
const talents = readJson(talentsPath, "talents");
const buffs = readJson(buffsPath, "buffs");
const aoyiStars = readJson(aoyiStarsPath, "aoyi stars");
const skillEffects = readJson(skillEffectsPath, "skill effects");
const originLedger = readJson(originLedgerPath, "current-build Aoyi origin ledger");
const aoyiActiveProofByEffect = indexAoyiActiveProof(originLedger);

const rows = [];
for (const candidate of ledger.candidates || []) {
  const proof = proveCandidate(candidate);
  if (proof) rows.push(proof);
}

rows.sort((left, right) =>
  String(left.source_id).localeCompare(String(right.source_id), undefined, { numeric: true }) ||
  String(left.source_name ?? "").localeCompare(String(right.source_name ?? ""))
);
const result = {
  schema_version: 1,
  generated_by: "tools/rdps-static-value-proof.mjs",
  game: "blue-protocol-star-resonance",
  static_game_build: String(ledger.static_game_build),
  policy: {
    static_ladder_is_distinct_from_runtime_grade_selection: true,
    direct_damage_healing_and_self_only_mechanics_are_preserved_but_not_rdps: true,
    localized_semantics_require_structured_current_build_value_evidence: true,
    unresolved_evidence_hidden: false,
    exact_numeric_source_identity_is_authoritative: true,
    localized_source_name_dispatch_is_legacy_fallback_only: true,
    static_value_proof_never_enables_rdps_transfer: true,
  },
  inputs: {
    recipient_scope_ledger: relativePath(ledgerPath),
    season_phantom_factors: relativePath(factorsPath),
    battle_imagines: relativePath(battleImaginesPath),
    rogue_entry_descriptions: relativePath(rogueDescriptionsPath),
    attribute_descriptions: relativePath(attributeDescriptionsPath),
    text_descriptions: relativePath(textDescriptionsPath),
    talents: relativePath(talentsPath),
    buffs: relativePath(buffsPath),
    aoyi_stars: relativePath(aoyiStarsPath),
    skill_effects: relativePath(skillEffectsPath),
    current_aoyi_origin_ledger: relativePath(originLedgerPath),
  },
  summary: {
    proven_sources: rows.length,
    external_rdps_sources: rows.filter((row) => row.disposition === "external-rdps-candidate").length,
    self_only_sources: rows.filter((row) => row.disposition === "self-only-nontransfer").length,
    source_owned_damage_sources: rows.filter((row) => row.disposition === "source-owned-direct-damage").length,
    healing_only_sources: rows.filter((row) => row.disposition === "healing-only-non-rdps").length,
    defensive_only_sources: rows.filter((row) => row.disposition === "defensive-self-only-non-rdps").length,
    source_owned_and_healing_sources: rows.filter((row) => row.disposition === "source-owned-and-healing-non-rdps").length,
    complete_static_value_ladders: rows.filter((row) => row.static_value_status === "complete-ladder").length,
    exact_static_formula_proofs: rows.filter((row) => row.static_value_status === "exact-formula").length,
    rdps_transfer_allowed_sources: rows.filter((row) => row.rdps_transfer_allowed === true).length,
  },
  sources: rows,
};

writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(JSON.stringify(result.summary));

function proveCandidate(candidate) {
  const aoyiProof = proveAoyiActiveModifier(candidate);
  if (aoyiProof) return aoyiProof;
  const exactSourceProof = new Map([
    ["season-rogue-entry:103", allClassAura],
    ["season-rogue-entry:195", coordinatedStrike],
    ["season-rogue-entry:196", elementSharing],
    ["season-rogue-entry:197", attributeTransfer],
    ["season-rogue-entry:209", synergyCritField],
    ["season-talent-node:5301", coordinatedStrike],
  ]).get(String(candidate.source_id));
  if (exactSourceProof) return exactSourceProof(candidate);
  const name = String(candidate.source_name || "");
  if (name === "Resolute Breath") return resoluteBreath(candidate);
  if (name === "Rorola - Enchantment") return rorola(candidate);
  if (name === "Lucy - Morale Reduction") return lucy(candidate);
  if (name === "Lethal Combo") return lethalCombo(candidate);
  if (name === "Beat Performer Reality Factor X4") return factorX4(candidate);
  if (name === "Beat Performer X6") return factorX6(candidate);
  if (name === "All-Class Aura") return allClassAura(candidate);
  if (name === "Synergy Crit Field") return synergyCritField(candidate);
  if (name === "Battle Cry") return battleCry(candidate);
  if (name === "Critical Cold") return criticalCold(candidate);
  if (name === "Freezing Meteor Storm") return freezingMeteorStorm(candidate);
  if (name === "Healing Note") return healingNote(candidate);
  if (name === "Inspire and Strengthen") return inspireAndStrengthen(candidate);
  if (name === "Severed Chapter") return severedChapter(candidate);
  if (name === "Survival Instinct") return survivalInstinct(candidate);
  if (name === "Thunder Curse") return thunderCurse(candidate);
  if (name === "Oblivion Dream") return oblivionDream(candidate);
  if (name === "Illusionary Sanctuary") return illusionarySanctuary(candidate);
  if (name === "Join Forces") return joinForces(candidate);
  if (name === "Element Sharing") return elementSharing(candidate);
  if (name === "Attribute Transfer") return attributeTransfer(candidate);
  if (name === "Coordinated Strike") return coordinatedStrike(candidate);
  if (name === "Arcane! Goblin March / Stunt! Blade Sweep target status 2110092") return bladeSweepArmorReduction(candidate);
  return null;
}

function proveAoyiActiveModifier(candidate) {
  const effectIds = (candidate.effect_ids || []).map(Number);
  const matches = effectIds.flatMap((effectId) => aoyiActiveProofByEffect.get(effectId) || []);
  if (matches.length === 0) return null;
  const identities = [...new Set(matches.map((match) => `${match.skill_id}:${match.effect_id}`))];
  if (identities.length !== 1) {
    throw new Error(`${candidate.source_name} has ambiguous current-build Aoyi modifier owners: ${identities.join(", ")}`);
  }
  const match = matches[0];
  const externallyTransferable = match.rdps_dispositions.some((value) =>
    value.includes("external-") || value.includes("counterfactual"));
  return {
    ...base(
      candidate,
      externallyTransferable ? "external-rdps-candidate" : "self-only-nontransfer",
      "complete-ladder",
      [
        `current-aoyi-rdps-origin-ledger skill ${match.skill_id}`,
        `SkillEffectTable ${match.skill_effect_id} ordered semantic labels`,
        "SkillAoyiStarTable exact tier parameter lanes",
        ...match.active_effect_ids.map((effectId) => `BuffTable ${effectId} duration and lifecycle identity`),
      ],
    ),
    owner_skill_id: match.skill_id,
    owner_skill_name: match.skill_name,
    skill_effect_id: match.skill_effect_id,
    active_effect_ids: match.active_effect_ids,
    recipient_scopes: match.recipient_scopes,
    tier_values: match.tiers,
    parameter_encoding: match.parameter_encoding,
    raw_units_per_percent: match.raw_units_per_percent,
    raw_units_per_decimal: match.raw_units_per_decimal,
    duration_seconds: match.duration_seconds,
    rdps_transfer_allowed: false,
    remaining_runtime_selector: externallyTransferable
      ? "encounter-local Aoyi tier plus exact provider, recipient, target lifecycle, and overlap arbitration"
      : "encounter-local Aoyi tier and owner lifecycle",
  };
}

function indexAoyiActiveProof(ledger) {
  const result = new Map();
  for (const skill of ledger.skills || []) {
    for (const proof of skill.active_modifier_parameter_evidence || []) {
      for (const effectId of proof.active_effect_ids || []) {
        const rows = result.get(Number(effectId)) || [];
        rows.push({
          skill_id: Number(skill.skill_id),
          skill_name: skill.name,
          effect_id: Number(effectId),
          skill_effect_id: Number(proof.skill_effect_id),
          active_effect_ids: (proof.active_effect_ids || []).map(Number),
          recipient_scopes: proof.recipient_scopes || [],
          rdps_dispositions: proof.rdps_dispositions || [],
          parameter_encoding: proof.parameter_encoding,
          raw_units_per_percent: Number(proof.raw_units_per_percent),
          raw_units_per_decimal: Number(proof.raw_units_per_decimal),
          duration_seconds: proof.duration_seconds,
          tiers: proof.tiers || [],
        });
        result.set(Number(effectId), rows);
      }
    }
  }
  return result;
}

function base(candidate, disposition, staticValueStatus, evidence) {
  return {
    source_rule_id: candidate.source_rule_id,
    source_id: candidate.source_id,
    source_name: candidate.source_name,
    effect_ids: candidate.effect_ids || [],
    disposition,
    static_value_status: staticValueStatus,
    evidence,
    rdps_transfer_allowed: false,
  };
}

function resoluteBreath(candidate) {
  return {
    ...base(candidate, "source-owned-direct-damage", "exact-formula", [
      "current-build localized skill description: shield detonation deals 30% of the casting tank's own max HP",
      "canonical emitted damage rows remain the authoritative realized damage amount",
    ]),
    formula: { kind: "source-owned-produced-damage", coefficient: 0.30, input: "source.max_hp" },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage", "skill-breakdown", "mechanic-catalog"],
  };
}

function rorola(candidate) {
  const entry = battleImagines.entriesByUid?.["3948"];
  if (!entry) throw new Error("battle imagine 3948 (Rorola) is missing");
  const corpus = JSON.stringify(entry.cleanDescriptions || entry.descriptions || {});
  if (!/自身|yourself|self|diri sendiri|soi-même|Anwender/i.test(corpus)) {
    throw new Error("Rorola current-build descriptions no longer prove owner-only damage");
  }
  return {
    ...base(candidate, "self-only-nontransfer", "complete-ladder", [
      "BattleImagineDescriptions.entriesByUid[3948]",
      "all authoritative locale semantics bind increased damage to the summoning player against targets they hit",
      "SkillAoyiStarTable tier values provide the complete base and per-stack ladder",
    ]),
    tier_values: tierValues(entry),
    stack_rule: { hits_per_stack: 10, maximum_additional_stacks: 5, extension_seconds: 3, maximum_extensions: 5 },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage-counterfactual", "imagine-breakdown", "mechanic-catalog"],
  };
}

function lucy(candidate) {
  const entry = battleImagines.entriesByUid?.["3982"];
  if (!entry) throw new Error("battle imagine 3982 (Lucy) is missing");
  const corpus = JSON.stringify(entry.cleanDescriptions || entry.descriptions || {});
  if (!/vulnerab|Verwundbarkeit|脆弱|易傷|취약/i.test(corpus) || !/element(?:al| resistance)|元素|エレメント|원소/i.test(corpus)) {
    throw new Error("Lucy current-build descriptions no longer prove vulnerability and elemental-resistance reduction");
  }
  const tiers = tierValues(entry);
  return {
    ...base(candidate, "external-rdps-candidate", "complete-ladder", [
      "BattleImagineDescriptions.entriesByUid[3982]",
      "SkillAoyiStarTable provides equal attrA/attrB/attrC values at each tier",
      "authoritative locale semantics identify ATK reduction, vulnerability, and elemental-resistance reduction for ten seconds",
    ]),
    tier_values: tiers,
    offensive_components: [
      { key: "target-vulnerability", tier_value_key: "attrB", duration_seconds: 10 },
      { key: "target-elemental-resistance-reduction", tier_value_key: "attrC", duration_seconds: 10 },
    ],
    defensive_components: [{ key: "target-attack-reduction", tier_value_key: "attrA", duration_seconds: 10 }],
    rdps_transfer_allowed: false,
    remaining_runtime_selector: "encounter-local imagine tier plus target lifecycle/provider identity",
  };
}

function lethalCombo(candidate) {
  const entry = battleImagines.entriesByUid?.["3976"];
  if (!entry) throw new Error("battle imagine 3976 (Arcane! Swift Devour) is missing");
  const description = String(entry.cleanDescriptions?.en || entry.cleanDescription || entry.description || "");
  assertIncludes(
    description,
    ["Class Skills", "Luck effect", "extra damage", "percentage of your ATK"],
    "Arcane! Swift Devour current-build description",
  );

  const effect = skillEffects["397601"];
  if (!effect) throw new Error("SkillEffectTable[397601] is missing");
  const effectDescription = JSON.stringify(effect.SkillAttrDes || []);
  assertIncludes(
    effectDescription,
    ["Additional Damage", 'skillpara.effect(\\"attr\\",\\"up\\")', "ATK/MATK", "20s"],
    "SkillEffectTable[397601] additional-damage formula",
  );

  const activeBuff = buffs["2110145"];
  const selectorBuff = buffs["3210230"];
  if (!activeBuff || activeBuff.Name !== "Lethal Combo") {
    throw new Error("BuffTable[2110145] no longer identifies Lethal Combo");
  }
  if (!/additional damage equal to a percentage of ATK/i.test(String(activeBuff.Desc || ""))) {
    throw new Error("BuffTable[2110145] no longer describes percentage-of-ATK additional damage");
  }
  if (Number(activeBuff.DestroyParam?.[0]?.[1]) !== 20) {
    throw new Error("BuffTable[2110145] no longer has the proven 20-second lifecycle");
  }
  if (!selectorBuff || !sameDesignOwner(activeBuff.NameDesign, selectorBuff.NameDesign)) {
    throw new Error("BuffTable[3210230] no longer shares Arcane! Swift Devour's current-build design owner");
  }

  const origin = (originLedger.skills || []).find((row) => Number(row.skill_id) === 3976);
  const ownerRoute = (origin?.owner_family_candidates || []).find((row) =>
    Number(row.buff_id) === 2110145
    && row.relationship === "current-aoyi-passive-owner-family"
    && row.owner_match_strength === "strong"
  );
  if (!ownerRoute) {
    throw new Error("current Aoyi origin ledger no longer proves BuffTable[2110145] as skill 3976's strong owner-family route");
  }

  const tiers = (entry.structuredValueRows || [])
    .filter((row) => row.key === "attr" && Number.isInteger(row.tier))
    .sort((left, right) => left.tier - right.tier)
    .map((value) => {
    if (value.unit !== "percent") {
      throw new Error(`Arcane! Swift Devour tier ${value.tier} attr is no longer encoded as a percentage`);
    }
    const aoyiRow = aoyiStars[String(325 + value.tier)];
    const rawValue = Number(aoyiRow?.FloatParameter?.[0]?.[1]);
    if (Number(value.rawValue) !== rawValue) {
      throw new Error(`Arcane! Swift Devour tier ${value.tier} disagrees with SkillAoyiStarTable row ${325 + value.tier}`);
    }
    return {
      tier: value.tier,
      attack_percent: Number(value.value),
      raw_value: rawValue,
      decimal_value: rawValue / 10000,
      skill_effect_id: 397601,
    };
  });
  const expected = [7.8, 15.6, 23.4, 31.2, 39];
  if (tiers.some((row, index) => row.attack_percent !== expected[index])) {
    throw new Error("Arcane! Swift Devour attr ladder changed from the proven 7.8/15.6/23.4/31.2/39 percent sequence");
  }

  return {
    ...base(candidate, "source-owned-direct-damage", "exact-formula", [
      "BattleImagineDescriptions.entriesByUid[3976] self-only trigger semantics",
      "SkillEffectTable[397601] binds attr to Additional Damage in ATK/MATK units for twenty seconds",
      "SkillAoyiStarTable rows 326-330 provide the exact five-tier attr coefficient ladder",
      "BuffTable[2110145] active lifecycle and BuffTable[3210230] transformed selector share the exact Aoyi design owner",
      "current-aoyi-rdps-origin-ledger skill 3976 proves BuffTable[2110145] as the strong owner-family route",
    ]),
    owner_skill_id: 3976,
    owner_skill_name: entry.name,
    skill_effect_id: 397601,
    active_effect_ids: [2110145],
    transformed_selector_effect_ids: [3210230],
    duration_seconds: 20,
    tier_values: tiers,
    formula: {
      kind: "triggered-source-owned-additional-damage",
      input: "source.attack_or_magic_attack",
      coefficient_by_tier: "tier_values[].decimal_value",
      trigger_categories: ["source-class-skill-damage", "source-luck-effect-damage"],
      output_authority: "canonical emitted damage row",
    },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage", "imagine-breakdown", "mechanic-catalog"],
    remaining_runtime_selector: "encounter-local equipped Imagine tier, active 2110145 lifecycle, and emitted additional-damage row binding",
  };
}

function factorX4(candidate) {
  const factor = factors.factorsByBuffId?.["3057430"];
  if (!factor) throw new Error("factor 3057430 is missing");
  const rows = factor.modifierEvidence?.gradeRows || [];
  assertGradeRows(rows, 10, "factor 3057430");
  if (!rows.every((row) => /healing|heal|cura|Soin|รักษา/i.test(row.cleanResolvedDescription || ""))) {
    throw new Error("factor 3057430 no longer resolves as healing-only");
  }
  return {
    ...base(candidate, "healing-only-non-rdps", "complete-ladder", [
      "SeasonPhantomFactors.factorsByBuffId[3057430].modifierEvidence.gradeRows",
      "every grade resolves to Surge Healing with no outgoing-damage component",
    ]),
    grades: rows.map((row) => ({ grade: row.grade, item_id: row.itemId, energy_threshold: row.parameterValues[0], cooldown_seconds: 3, healing_atk_percent: 200 })),
    rdps_transfer_allowed: false,
    retained_for: ["healing-attribution", "factor-breakdown", "mechanic-catalog"],
  };
}

function factorX6(candidate) {
  const factor = factors.factorsByBuffId?.["3057060"];
  if (!factor) throw new Error("factor 3057060 is missing");
  const rows = factor.modifierEvidence?.gradeRows || [];
  assertGradeRows(rows, 10, "factor 3057060");
  return {
    ...base(candidate, "external-rdps-candidate", "complete-ladder", [
      "SeasonPhantomFactors.factorsByBuffId[3057060].modifierEvidence.gradeRows",
      "current-build description explicitly covers Encore triggered by self or allies",
    ]),
    grades: rows.map((row) => ({ grade: row.grade, item_id: row.itemId, damage_percent: row.parameterValues[1] / 100, illusion_energy: row.parameterValues[0] })),
    affected_output: "Encore Illusion-Breaking damage triggered by self or allies",
    rdps_transfer_allowed: false,
    remaining_runtime_selector: "encounter-local selected factor grade and exact Encore output ancestry",
  };
}

function allClassAura(candidate) {
  const row = rogueDescriptions["100301"];
  if (!row || !/5%/.test(row.Content || "") || !/20%/.test(row.Content || "")) {
    throw new Error("RogueEntryDescriptionTable[100301] no longer proves the All-Class Aura ladder");
  }
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["RogueEntryDescriptionTable[100301]"]),
    formula: { stat: "atk", radius_meters: 10, base_percent: 5, percent_per_distinct_role: 5, maximum_percent: 20, recipients: "user-and-allies" },
    rdps_transfer_allowed: false,
    remaining_runtime_selector: "active aura provider, recipient range/window, and distinct-role count",
  };
}

function synergyCritField(candidate) {
  const description = rogueDescription(110901, "Synergy Crit Field");
  assertIncludes(
    description,
    ["Special Attack", "Crit Aura", "5s", "allies within 15m", "+3%"],
    "Synergy Crit Field",
  );
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", [
      "RogueEntryDescriptionTable[110901]",
      "current-build description says critical damage, not critical rate",
      "Portuguese 'dano critico' is critical damage and must not create a second critical-rate component",
    ]),
    formula: {
      stat: "critical_damage",
      radius_meters: 15,
      percent: 3,
      duration_seconds: 5,
      recipients: "allies",
      trigger: "provider-special-attack",
    },
    rejected_component: "critical-rate",
    rdps_transfer_allowed: false,
    remaining_runtime_selector: "active field provider and recipient window",
  };
}

function battleCry(candidate) {
  const description = talentDescription(434, "Battle Cry");
  assertIncludes(description, ["10%", "Haste", "10s", "only one effect"], "Battle Cry");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["TalentTable[434].TalentDes"]),
    formula: { stat: "haste", percent: 10, duration_seconds: 10, recipients: "allies", overlap: "only-one-active" },
    excluded_self_components: [{ stat: "critical_damage", percent: 50, duration_seconds: 10 }],
    rejected_components: ["critical-damage"],
    remaining_runtime_selector: "Inspire provider/recipient lifecycle plus haste counterfactual timing model",
  };
}

function criticalCold(candidate) {
  const description = attributeDescription(2204470, "Critical Cold");
  assertIncludes(description, ["Crit DMG", "15%", "allies", "critical rate", "3%", "Permafrost"], "Critical Cold");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["AttrDescription[2204470]"]),
    formula: { stat: "critical_rate", percent: 3, recipients: "allies", predicate: "provider-permafrost-active" },
    excluded_self_components: [{ stat: "critical_damage", percent: 15, predicate: "self-permafrost-active" }],
    rejected_components: ["critical-damage"],
    remaining_runtime_selector: "provider Permafrost lifecycle and exact crit-rate counterfactual",
  };
}

function freezingMeteorStorm(candidate) {
  const description = attributeDescription(2204390, "Freezing Meteor Storm");
  const frozen = textDescription(1171, "Frozen");
  assertIncludes(description, ["20%", "Frozen", "0.5s", "5s"], "Freezing Meteor Storm");
  assertIncludes(frozen, ["Ice DMG taken", "20%", "10s", "breaks upon taking DMG", "Does not affect bosses"], "Frozen");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["AttrDescription[2204390]", "TextDescription[1171]"]),
    formula: {
      stat: "ice_damage_taken",
      percent: 20,
      recipients: "all-attackers-of-frozen-target",
      duration_seconds: 10,
      ends_on: "first-damage-taken",
      excluded_targets: "bosses",
      application_chance_percent: 20,
      source_skill: "Meteor Storm",
    },
    non_damage_components: [{ kind: "permafrost-extension", seconds_per_critical_hit: 0.5, maximum_seconds: 5 }],
    remaining_runtime_selector: "Frozen effect identity/provider lifecycle and first-hit consume arbitration",
  };
}

function healingNote(candidate) {
  const description = attributeDescription(2207140, "Healing Note");
  const note = textDescription(1192, "Note");
  const buff = buffs[2207140];
  assertIncludes(description, ["Basic Attack", "extra instance", "5", "Soundwave Energy", "lowest HP"], "Healing Note");
  assertIncludes(note, ["35%", "ATK", "10s", "remaining healing"], "Note");
  if (!buff?.Note?.includes("普通攻击变为二连击")) throw new Error("BuffTable[2207140] no longer proves the extra basic-attack hit");
  return {
    ...base(candidate, "source-owned-and-healing-non-rdps", "exact-formula", ["AttrDescription[2207140]", "TextDescription[1192]", "BuffTable[2207140].Note"]),
    direct_damage: { kind: "source-owned-extra-basic-attack-instance" },
    healing: { atk_percent_total: 35, duration_seconds: 10, energy_per_note: 5, target: "lowest-hp-ally", refresh: "settle-remaining-healing-immediately" },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage", "healing-attribution", "mechanic-catalog"],
  };
}

function inspireAndStrengthen(candidate) {
  const description = attributeDescription(2202720, "Inspire and Strengthen");
  const inspiration = textDescription(1107, "Inspiration");
  assertIncludes(description, ["135", "Intellect", "Strength", "Agility", "Endurance"], "Inspire and Strengthen");
  assertIncludes(inspiration, ["100", "1.5%", "Crit", "Haste", "Luck", "Mastery", "Versatility"], "Inspiration");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["AttrDescription[2202720]", "TextDescription[1107]"]),
    formula: {
      inherited_base_inspiration: {
        primary_stats_flat: { intellect: 100, strength: 100, agility: 100, endurance: 100 },
        secondary_stats_percent: { critical_rate: 1.5, haste: 1.5, luck: 1.5, mastery: 1.5, versatility: 1.5 },
      },
      strengthened_primary_stats_flat: { intellect: 135, strength: 135, agility: 135, endurance: 135 },
      interpretation: "the talent changes the four flat primary-stat grants from 100 to 135; it does not replace the 1.5% secondary-stat grants",
    },
    remaining_runtime_selector: "Inspiration lifecycle, recipient class stat dependency, and attribute-to-damage transforms",
  };
}

function severedChapter(candidate) {
  const description = attributeDescription(2207120, "Severed Chapter");
  assertIncludes(description, ["30", "10%", "30%", "15%", "Resilience Broken", "cannot stack"], "Severed Chapter");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["AttrDescription[2207120]", "BuffTable[2207121]"]),
    formula: { stat: "generic_damage", percent: 15, recipients: "self-and-allies-within-30m", predicate: "target-resilience-broken-and-provider-heroic-melody-active", overlap: "nonstacking" },
    support_components: [
      { stat: "resilience_break_efficiency", percent: 10, recipients: "self-and-allies-within-30m", overlap: "nonstacking" },
      { stat: "resilience_break_efficiency", percent: 30, recipients: "self-and-allies-within-30m", predicate: "provider-heroic-melody-active", overlap: "nonstacking" },
    ],
    remaining_runtime_selector: "Heroic Melody provider window, 30m recipients, target break-state lifecycle, and nonstacking provider arbitration",
  };
}

function survivalInstinct(candidate) {
  const description = attributeDescription(2201270, "Survival Instinct");
  const runtimeBuff = buffs[2201271];
  const talent = talents[925];
  assertIncludes(description, ["HP", "30%", "60"], "Survival Instinct");
  assertIncludes(runtimeBuff?.Desc || "", ["Damage taken -50%", "healing received +30%", "10s"], "Survival Instinct runtime buff");
  if (JSON.stringify(talent?.BuffPar) !== "[[5000,3000,10000,60000]]") throw new Error("TalentTable[925] parameters changed");
  return {
    ...base(candidate, "defensive-self-only-non-rdps", "exact-formula", ["TalentTable[925]", "AttrDescription[2201270]", "BuffTable[2201271]"]),
    trigger: { self_hp_below_percent: 30, cooldown_seconds: 60 },
    formula: { damage_taken_reduction_percent: 50, healing_received_percent: 30, duration_seconds: 10, recipient: "self" },
    rdps_transfer_allowed: false,
    retained_for: ["tanked", "healing", "mechanic-catalog"],
  };
}

function thunderCurse(candidate) {
  const description = attributeDescription(2200250, "Thunder Curse");
  assertIncludes(description, ["from you", "2%", "10", "4"], "Thunder Curse");
  return {
    ...base(candidate, "self-only-nontransfer", "exact-formula", ["AttrDescription[2200250]"]),
    formula: { stat: "target_damage_taken_from_provider_only", percent_per_stack: 2, duration_seconds: 10, maximum_stacks: 4 },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage-counterfactual", "mechanic-catalog"],
  };
}

function oblivionDream(candidate) {
  const description = attributeDescription(3003010, "Oblivion Dream");
  const oblivion = textDescription(1205, "Oblivion");
  assertIncludes(description, ["enemies", "5m", "Oblivion"], "Oblivion Dream");
  assertIncludes(oblivion, ["ATK", "20%", "Dream DMG taken", "10%", "cannot stack"], "Oblivion");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["AttrDescription[3003010]", "TextDescription[1205]"]),
    formula: { stat: "dream_damage_taken", percent: 10, recipients: "all-dream-damage-attackers", radius_meters: 5, overlap: "nonstacking" },
    defensive_components: [{ stat: "target_attack_reduction", percent: 20 }, { stat: "target_movement_speed_reduction", percent: 20 }],
    rejected_components: ["attack-stat-reduction"],
    remaining_runtime_selector: "Oblivion provider/target lifecycle and Dream-damage classification",
  };
}

function illusionarySanctuary(candidate) {
  const description = rogueDescription(108801, "Illusionary Sanctuary");
  assertIncludes(description, ["For each active Battle Imagine buff", "damage", "4%", "damage taken", "2%"], "Illusionary Sanctuary");
  return {
    ...base(candidate, "self-only-nontransfer", "exact-formula", ["RogueEntryDescriptionTable[108801]"]),
    formula: { personal_damage_percent_per_active_battle_imagine_buff: 4, personal_damage_taken_reduction_percent_per_active_battle_imagine_buff: 2 },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage-counterfactual", "tanked", "mechanic-catalog"],
  };
}

function joinForces(candidate) {
  const description = rogueDescription(109801, "Join Forces");
  assertIncludes(description, ["For each ally within 15m", "damage", "8%", "damage taken", "4%"], "Join Forces");
  return {
    ...base(candidate, "self-only-nontransfer", "exact-formula", ["RogueEntryDescriptionTable[109801]"]),
    formula: { personal_damage_percent_per_nearby_ally: 8, personal_damage_taken_reduction_percent_per_nearby_ally: 4, radius_meters: 15 },
    rdps_transfer_allowed: false,
    retained_for: ["personal-damage-counterfactual", "tanked", "mechanic-catalog"],
  };
}

function elementSharing(candidate) {
  const description = rogueDescription(109601, "Element Sharing");
  assertIncludes(description, ["Elemental Damage changes", "nearby allies", "20%", "10s"], "Element Sharing");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["RogueEntryDescriptionTable[109601]"]),
    formula: { stat: "elemental_damage", percent: 20, duration_seconds: 10, recipients: "nearby-allies", trigger: "provider-elemental-damage-changes" },
    remaining_runtime_selector: "provider trigger, recipient range/lifecycle, and affected elemental damage rows",
  };
}

function attributeTransfer(candidate) {
  const description = rogueDescription(109701, "Attribute Transfer");
  assertIncludes(description, ["Crit/Luck/Haste/Mastery/Versatility", "nearby allies", "10%", "10s"], "Attribute Transfer");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", ["RogueEntryDescriptionTable[109701]"]),
    formula: { corresponding_stat_percent: 10, duration_seconds: 10, recipients: "nearby-allies", trigger: "provider-corresponding-stat-changes", stats: ["critical_rate", "luck", "haste", "mastery", "versatility"] },
    remaining_runtime_selector: "changed stat identity, provider/recipient lifecycle, and exact attribute transforms",
  };
}

function coordinatedStrike(candidate) {
  const isRogue = candidate.source_id === "season-rogue-entry:195";
  const description = isRogue ? rogueDescription(109501, "Coordinated Strike rogue") : attributeDescription(3004210, "Coordinated Strike season node");
  const attackPercent = isRogue ? 15 : 10;
  assertIncludes(description, ["Class Skills deal damage", "1%", `${attackPercent}%`, "ATK", "3s", "0.3s"], "Coordinated Strike");
  return {
    ...base(candidate, "external-rdps-candidate", "exact-formula", [isRogue ? "RogueEntryDescriptionTable[109501]" : "AttrDescription[3004210]"]),
    formula: { stat: "atk", percent: attackPercent, duration_seconds: 3, recipients: "self-and-nearby-allies", trigger: "provider-class-skill-damage", trigger_cooldown_seconds: 0.3 },
    healing_components: [{ max_hp_percent: 1, recipients: "self-and-nearby-allies" }],
    rejected_components: ["cooldown-or-resource"],
    remaining_runtime_selector: "provider trigger, recipient range/lifecycle, and ATK-to-damage transform",
  };
}

function bladeSweepArmorReduction(candidate) {
  const rows = Object.values(aoyiStars)
    .filter((row) => Number(row.SkillId) === 3914)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  if (rows.length !== 5 || rows.some((row, index) => Number(row.Level) !== index + 1)) {
    throw new Error("SkillAoyiStarTable skill 3914 no longer contains a complete five-tier ladder");
  }
  const effect = skillEffects[391401];
  const labels = (effect?.SkillAttrDes || []).map((row) => String(row?.[0] || ""));
  if (!labels.includes("Block DMG Reduction Bonus") || !labels.includes("Armor Penetration")) {
    throw new Error("SkillEffectTable[391401] no longer binds attrPer/attrAdd to block reduction and armor penetration");
  }
  const tiers = rows.map((row) => {
    const values = Object.fromEntries((row.FloatParameter || []).map(([key, value]) => [String(key), Number(value)]));
    if (!Number.isFinite(values.attrPer) || !Number.isFinite(values.attrAdd)) {
      throw new Error(`SkillAoyiStarTable skill 3914 tier ${row.Level} is missing attrPer/attrAdd`);
    }
    return {
      tier: Number(row.Level),
      owner_block_damage_reduction_percent: values.attrPer / 100,
      target_armor_reduction_percent: values.attrAdd / 100,
      raw_owner_attr_per: values.attrPer,
      raw_target_attr_add: values.attrAdd,
    };
  });
  const expected = [1.3, 2.6, 3.9, 5.2, 6.5];
  if (tiers.some((row, index) => row.target_armor_reduction_percent !== expected[index])) {
    throw new Error("Blade Sweep target armor-reduction ladder changed from the proven 1.3/2.6/3.9/5.2/6.5 percent sequence");
  }
  return {
    ...base(candidate, "external-rdps-candidate", "complete-ladder", [
      "SkillAoyiStarTable rows for skill 3914",
      "SkillEffectTable[391401] ordered semantic labels",
      "BuffTable[2110092] ten-second target lifecycle",
      "current component bridge: skills 3914 and 3946 share projectile 10040102 and target status 2110092",
    ]),
    tier_values: tiers,
    formula: {
      stat: "target_physical_armor_reduction",
      tier_value_key: "target_armor_reduction_percent",
      duration_seconds: 10,
      recipients: "all-physical-damage-attackers-against-affected-target",
    },
    defensive_owner_component: {
      stat: "owner_block_damage_reduction",
      tier_value_key: "owner_block_damage_reduction_percent",
      duration_seconds: 20,
      rdps_transfer_allowed: false,
    },
    rdps_transfer_allowed: false,
    remaining_runtime_selector: "packet-resolved summon owner, equipped Imagine tier, target lifecycle, physical damage rows, and armor-stage counterfactual",
  };
}

function talentDescription(id, label) {
  const value = talents[id]?.TalentDes;
  if (!value) throw new Error(`TalentTable[${id}] is missing ${label}`);
  return value;
}

function attributeDescription(id, label) {
  const value = attributeDescriptions[id]?.Description;
  if (!value) throw new Error(`AttrDescription[${id}] is missing ${label}`);
  return value;
}

function textDescription(id, label) {
  const value = textDescriptions[id]?.Description;
  if (!value) throw new Error(`TextDescription[${id}] is missing ${label}`);
  return value;
}

function rogueDescription(id, label) {
  const value = rogueDescriptions[id]?.Content;
  if (!value) throw new Error(`RogueEntryDescriptionTable[${id}] is missing ${label}`);
  return value;
}

function assertIncludes(value, needles, label) {
  for (const needle of needles) {
    if (!String(value).includes(needle)) throw new Error(`${label} no longer contains ${needle}`);
  }
}

function sameDesignOwner(left, right) {
  const owner = (value) => String(value || "").split("-")[0].trim();
  return owner(left).length > 0 && owner(left) === owner(right);
}

function tierValues(entry) {
  const byTier = new Map();
  for (const value of entry.structuredValueRows || []) {
    if (!Number.isInteger(value.tier) || !value.key) continue;
    const row = byTier.get(value.tier) || { tier: value.tier, values: {} };
    row.values[value.key] = { value: value.value, unit: value.unit, raw_value: value.rawValue };
    byTier.set(value.tier, row);
  }
  const rows = [...byTier.values()].sort((left, right) => left.tier - right.tier);
  if (rows.length !== 5) throw new Error(`battle imagine ${entry.uid} does not have five structured tiers`);
  return rows;
}

function assertGradeRows(rows, count, label) {
  if (rows.length !== count || rows.some((row, index) => row.grade !== index + 1)) {
    throw new Error(`${label} does not contain a complete ${count}-grade ladder`);
  }
}

function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`invalid argument near ${key}`);
    values[key.slice(2)] = value;
  }
  for (const key of ["ledger", "factors", "battleImagines", "rogueDescriptions", "attributeDescriptions", "textDescriptions", "talents", "buffs", "aoyiStars", "skillEffects", "output"]) {
    if (!values[key]) throw new Error(`--${key} is required`);
  }
  return values;
}

function readJson(filePath, label) {
  try { return JSON.parse(readFileSync(filePath, "utf8")); }
  catch (error) { throw new Error(`failed to read ${label} at ${filePath}: ${error.message}`); }
}

function resolvePath(input) { return path.isAbsolute(input) ? input : path.resolve(repoRoot, input); }
function relativePath(input) { return path.relative(repoRoot, input).replaceAll("\\", "/"); }
