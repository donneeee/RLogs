#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const SCHEMA_VERSION = 2;
const GENERATOR = "tools/bpsr-selected-pair-formula-context.mjs";
const BUILD_ID = "24687926";
const COHORT_SCHEMA_VERSION = 43;
const SESSION_ID = "monitor-1787014465500.run-0001";
const ACTION_ID = 2_203_291;
const PRESENT_SEQUENCE = 55_702;
const ABSENT_SEQUENCE = 57_683;
const PHY_BOOST_ATTRIBUTE_ID = 12_550;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);
if (command === "build") await build(options);
else if (command === "verify") verifyCommand(options);
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function build(parsed) {
  const cohortPath = resolved(parsed, "cohort");
  const output = path.resolve(required(parsed, "output"));
  if (existsSync(output)) throw new Error(`Refusing to overwrite existing output: ${output}`);
  const report = await buildReport(cohortPath);
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  mkdirSync(path.dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(`wrote ${output}`);
}

async function buildReport(cohortPath) {
  const header = readHeader(cohortPath);
  assert(header.schema_version === COHORT_SCHEMA_VERSION && header.game_build === BUILD_ID,
    "Formula cohort identity mismatch");

  const samples = [];
  const sampleScan = await scanObjectArray(
    cohortPath,
    "samples",
    (sample) => sample?.session_id === SESSION_ID &&
      sample?.ability_id === ACTION_ID &&
      [PRESENT_SEQUENCE, ABSENT_SEQUENCE].includes(sample?.sequence),
    (sample) => samples.push(sample),
  );
  samples.sort((left, right) => left.sequence - right.sequence);
  assert(samples.length === 2 && samples[0].sequence === PRESENT_SEQUENCE &&
    samples[1].sequence === ABSENT_SEQUENCE, "Selected formula samples changed");
  assert(samples[0].amount === 137_832 && samples[1].amount === 131_206 &&
    samples.every((sample) => sample.source_entity_uuid === 216_009_015_936 &&
      sample.target_entity_uuid === 7_086_997_568 && sample.hit_event_id === 9),
  "Selected formula sample identity changed");

  const sourceStateIds = new Set(samples.map((sample) => {
    const value = Number(sample.source_attribute_state_id);
    assert(Number.isSafeInteger(value) && value >= 0, "Invalid source attribute state id");
    return value;
  }));
  const targetStateIds = new Set(samples.map((sample) => {
    const value = Number(sample.target_attribute_state_id);
    assert(Number.isSafeInteger(value) && value >= 0, "Invalid target attribute state id");
    return value;
  }));
  const stateIds = new Set([...sourceStateIds, ...targetStateIds]);
  const states = new Map();
  const stateScan = await scanSelectedArrayItems(
    cohortPath,
    "attribute_states",
    stateIds,
    (index, text) => states.set(index, JSON.parse(text)),
  );
  assert(states.size === stateIds.size, "Not all selected source/target attribute states were retained");

  const selectedSamples = samples.map((sample) => ({
    session_id: sample.session_id,
    sequence: sample.sequence,
    observed_micros: sample.observed_micros,
    wire_capture_sequence: sample.wire_capture_sequence,
    scene_id: sample.scene_id,
    source_entity_uuid: sample.source_entity_uuid,
    direct_source_entity_uuid: sample.direct_source_entity_uuid,
    target_entity_uuid: sample.target_entity_uuid,
    ability_id: sample.ability_id,
    hit_event_id: sample.hit_event_id,
    amount: sample.amount,
    critical: sample.critical,
    lucky: sample.lucky,
    source_attribute_state_id: sample.source_attribute_state_id,
    source_status_state_id: sample.source_status_state_id,
    target_attribute_state_id: sample.target_attribute_state_id,
    target_status_state_id: sample.target_status_state_id,
  }));
  const retainedStates = (selectedIds) => [...selectedIds]
    .sort((left, right) => left - right)
    .map((stateId) => ({ state_id: stateId, attributes: states.get(stateId) }));
  const sourceAttributeStates = retainedStates(sourceStateIds);
  const targetAttributeStates = retainedStates(targetStateIds);
  const phyBoost = sourceAttributeStates.map((state) => ({
    state_id: state.state_id,
    value: attributeValue(state.attributes, PHY_BOOST_ATTRIBUTE_ID),
  }));
  assert(phyBoost.every((row) => row.value === 600),
    "Selected source PHY Boost state changed");
  const presentTargetState = states.get(Number(samples[0].target_attribute_state_id));
  const absentTargetState = states.get(Number(samples[1].target_attribute_state_id));
  const targetAttributeChanges = attributeChanges(presentTargetState, absentTargetState);

  return {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATOR,
    game_build: BUILD_ID,
    selection: {
      session_id: SESSION_ID,
      action_id: ACTION_ID,
      present_sequence: PRESENT_SEQUENCE,
      absent_sequence: ABSENT_SEQUENCE,
    },
    input: {
      path: cohortPath.replaceAll("\\", "/"),
      bytes: statSync(cohortPath).size,
      sha256: sampleScan.sha256,
      schema_version: header.schema_version,
      game_build: header.game_build,
    },
    policy: {
      exact_numeric_ids_and_build_identity_authoritative: true,
      cohort_streamed_not_fully_deserialized: true,
      only_selected_samples_and_referenced_attribute_states_retained: true,
      current_character_snapshot_substitution_allowed: false,
      attribute_presence_proves_formula_stage: false,
      provider_rdps_credit_allowed: false,
    },
    scans: {
      samples: sampleScan,
      attribute_states: stateScan,
    },
    selected_samples: selectedSamples,
    source_attribute_states: sourceAttributeStates,
    target_attribute_states: targetAttributeStates,
    target_attribute_state_comparison: {
      present_state_id: Number(samples[0].target_attribute_state_id),
      absent_state_id: Number(samples[1].target_attribute_state_id),
      exact_attribute_vectors_equal: targetAttributeChanges.length === 0,
      changed_attributes: targetAttributeChanges,
      exact_formula_effect_of_each_change_proven: false,
    },
    exact_selected_source_attribute_context: {
      attribute_id: PHY_BOOST_ATTRIBUTE_ID,
      internal_name: "AttrDamInc",
      raw_values_by_state: phyBoost,
      all_selected_states_equal_raw_600: true,
      raw_to_display_percent_divisor: 100,
      display_percent: 6,
      exact_formula_stage_proven: false,
      exact_integer_operation_and_rounding_proven: false,
    },
    summary: {
      selected_samples: selectedSamples.length,
      distinct_source_attribute_states: sourceAttributeStates.length,
      distinct_target_attribute_states: targetAttributeStates.length,
      target_attribute_changes: targetAttributeChanges.length,
      selected_source_phy_boost_raw: 600,
      selected_source_phy_boost_display_percent: 6,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
  };
}

async function scanObjectArray(file, propertyName, predicate, onMatch) {
  const marker = `"${propertyName}":[`;
  let markerOffset = 0;
  let found = false;
  let complete = false;
  let itemStarted = false;
  let depth = 0;
  let inString = false;
  let escaped = false;
  let itemText = "";
  let itemsScanned = 0;
  let retainedItems = 0;
  let maximumCapturedItemBytes = 0;
  let bytesRead = 0;
  const hash = createHash("sha256");
  const stream = createReadStream(file, { encoding: "utf8", highWaterMark: 1024 * 1024 });
  for await (const chunk of stream) {
    bytesRead += Buffer.byteLength(chunk);
    hash.update(chunk);
    for (const character of chunk) {
      if (!found) {
        if (character === marker[markerOffset]) {
          markerOffset += 1;
          if (markerOffset === marker.length) found = true;
        } else markerOffset = character === marker[0] ? 1 : 0;
        continue;
      }
      if (complete) continue;
      if (!itemStarted) {
        if (/\s|,/.test(character)) continue;
        if (character === "]") {
          complete = true;
          continue;
        }
        assert(character === "{", `Expected ${propertyName} object, got ${character}`);
        itemStarted = true;
        depth = 1;
        itemText = character;
        continue;
      }
      itemText += character;
      if (inString) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === "\"") inString = false;
        continue;
      }
      if (character === "\"") inString = true;
      else if (character === "{" || character === "[") depth += 1;
      else if (character === "}" || character === "]") {
        depth -= 1;
        if (depth === 0) {
          const sample = JSON.parse(itemText);
          maximumCapturedItemBytes = Math.max(maximumCapturedItemBytes,
            Buffer.byteLength(itemText));
          if (predicate(sample)) {
            onMatch(sample);
            retainedItems += 1;
          }
          itemsScanned += 1;
          itemStarted = false;
          itemText = "";
        }
      }
    }
  }
  assert(found && complete, `Property ${propertyName} was not completely scanned`);
  return {
    property: propertyName,
    bytes_read: bytesRead,
    array_items_scanned: itemsScanned,
    retained_items: retainedItems,
    maximum_single_item_bytes: maximumCapturedItemBytes,
    bounded_item_retention: true,
    sha256: `sha256:${hash.digest("hex")}`,
  };
}

async function scanSelectedArrayItems(file, propertyName, selectedIndexes, onItem) {
  const marker = `"${propertyName}":[`;
  let markerOffset = 0;
  let found = false;
  let complete = false;
  let index = 0;
  let itemDepth = 0;
  let itemStarted = false;
  let inString = false;
  let escaped = false;
  let capture = false;
  let itemText = "";
  let bytesRead = 0;
  let maximumCapturedItemBytes = 0;
  let retainedItems = 0;
  const stream = createReadStream(file, { encoding: "utf8", highWaterMark: 1024 * 1024 });
  for await (const chunk of stream) {
    bytesRead += Buffer.byteLength(chunk);
    for (const character of chunk) {
      if (!found) {
        if (character === marker[markerOffset]) {
          markerOffset += 1;
          if (markerOffset === marker.length) found = true;
        } else markerOffset = character === marker[0] ? 1 : 0;
        continue;
      }
      if (complete) break;
      if (!itemStarted) {
        if (/\s|,/.test(character)) continue;
        if (character === "]") {
          complete = true;
          break;
        }
        assert(character === "[", `Expected ${propertyName}[${index}] array`);
        itemStarted = true;
        itemDepth = 1;
        capture = selectedIndexes.has(index);
        itemText = capture ? character : "";
        continue;
      }
      if (capture) itemText += character;
      if (inString) {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === "\"") inString = false;
        continue;
      }
      if (character === "\"") inString = true;
      else if (character === "[" || character === "{") itemDepth += 1;
      else if (character === "]" || character === "}") {
        itemDepth -= 1;
        if (itemDepth === 0) {
          if (capture) {
            maximumCapturedItemBytes = Math.max(maximumCapturedItemBytes,
              Buffer.byteLength(itemText));
            onItem(index, itemText);
            retainedItems += 1;
          }
          index += 1;
          itemStarted = false;
          itemText = "";
          capture = false;
          if ([...selectedIndexes].every((selected) => selected < index)) {
            complete = true;
            break;
          }
        }
      }
    }
    if (complete) break;
  }
  stream.destroy();
  assert(found, `Property ${propertyName} not found`);
  return {
    property: propertyName,
    bytes_read_through_last_requested_state: bytesRead,
    array_items_scanned: index,
    requested_items: selectedIndexes.size,
    retained_items: retainedItems,
    maximum_retained_item_bytes: maximumCapturedItemBytes,
    bounded_prefix_scan: true,
  };
}

function verifyCommand(parsed) {
  const input = resolved(parsed, "input");
  const report = JSON.parse(readFileSync(input, "utf8"));
  verifyReport(report);
  console.log(`verified ${input}`);
}

function verifyReport(report) {
  assert(report?.schema_version === SCHEMA_VERSION && report?.generated_by === GENERATOR &&
    report?.game_build === BUILD_ID, "Receipt identity mismatch");
  assert(report?.policy?.cohort_streamed_not_fully_deserialized === true &&
    report?.policy?.provider_rdps_credit_allowed === false,
  "Receipt policy mismatch");
  assert(report?.selected_samples?.length === 2 &&
    report?.target_attribute_states?.length >= 1 &&
    Array.isArray(report?.target_attribute_state_comparison?.changed_attributes) &&
    report?.target_attribute_state_comparison?.exact_formula_effect_of_each_change_proven === false &&
    report?.exact_selected_source_attribute_context?.attribute_id === PHY_BOOST_ATTRIBUTE_ID &&
    report?.exact_selected_source_attribute_context?.all_selected_states_equal_raw_600 === true &&
    report?.exact_selected_source_attribute_context?.exact_formula_stage_proven === false,
  "Selected PHY Boost context mismatch");
  assert(report?.summary?.formula_authority === false &&
    report?.summary?.provider_rdps_credit_allowed === false,
  "Receipt unexpectedly grants formula or rDPS authority");
  assert(report?.content_sha256 === contentHash(report), "Content hash mismatch");
}

async function selfTest() {
  const directory = mkdtempSync(path.join(tmpdir(), "rlogs-selected-pair-"));
  const cohort = path.join(directory, "cohort.json");
  try {
    const sample = (sequence, amount) => ({
      session_id: SESSION_ID,
      sequence,
      observed_micros: sequence,
      wire_capture_sequence: sequence,
      scene_id: 6525,
      source_entity_uuid: 216_009_015_936,
      direct_source_entity_uuid: null,
      target_entity_uuid: 7_086_997_568,
      ability_id: ACTION_ID,
      hit_event_id: 9,
      amount,
      critical: true,
      lucky: false,
      source_attribute_state_id: 1,
      source_status_state_id: 2,
      target_attribute_state_id: sequence === PRESENT_SEQUENCE ? 0 : 2,
      target_status_state_id: 3,
    });
    writeFileSync(cohort, JSON.stringify({
      schema_version: COHORT_SCHEMA_VERSION,
      game_build: BUILD_ID,
      attribute_states: [
        [{ attribute_id: 11350, value: 10 }],
        [{ attribute_id: PHY_BOOST_ATTRIBUTE_ID, value: 600 }],
        [{ attribute_id: 11350, value: 11 }],
      ],
      status_states: [],
      samples: [sample(PRESENT_SEQUENCE, 137_832), sample(ABSENT_SEQUENCE, 131_206)],
    }));
    const report = await buildReport(cohort);
    report.content_sha256 = contentHash(report);
    verifyReport(report);
    assert(report.scans.samples.array_items_scanned === 2 &&
      report.scans.attribute_states.retained_items === 3 &&
      report.target_attribute_state_comparison.changed_attributes.length === 1,
    "Self-test scan counts mismatch");
    console.log("self-test passed");
  } finally {
    if (existsSync(cohort)) unlinkSync(cohort);
    rmdirSync(directory);
  }
}

function attributeValue(attributes, id) {
  const row = (attributes ?? []).find((attribute) => Number(attribute.attribute_id) === id);
  const value = Number(row?.value);
  return Number.isSafeInteger(value) ? value : null;
}

function attributeChanges(presentAttributes, absentAttributes) {
  const present = new Map((presentAttributes ?? []).map((row) =>
    [Number(row.attribute_id), Number(row.value)]));
  const absent = new Map((absentAttributes ?? []).map((row) =>
    [Number(row.attribute_id), Number(row.value)]));
  return [...new Set([...present.keys(), ...absent.keys()])]
    .sort((left, right) => left - right)
    .filter((attributeId) => present.get(attributeId) !== absent.get(attributeId))
    .map((attributeId) => ({
      attribute_id: attributeId,
      present_value: present.has(attributeId) ? present.get(attributeId) : null,
      absent_value: absent.has(attributeId) ? absent.get(attributeId) : null,
      delta_present_minus_absent:
        present.has(attributeId) && absent.has(attributeId)
          ? present.get(attributeId) - absent.get(attributeId)
          : null,
    }));
}

function readHeader(file) {
  const descriptor = openSync(file, "r");
  try {
    const buffer = Buffer.alloc(65_536);
    const bytes = readSync(descriptor, buffer, 0, buffer.length, 0);
    const prefix = buffer.subarray(0, bytes).toString("utf8");
    return {
      schema_version: Number(/"schema_version":(\d+)/.exec(prefix)?.[1]),
      game_build: /"game_build":"([^"]+)"/.exec(prefix)?.[1] ?? null,
    };
  } finally {
    closeSync(descriptor);
  }
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return `sha256:${createHash("sha256").update(JSON.stringify(copy)).digest("hex")}`;
}

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    assert(key?.startsWith("--") && args[index + 1] !== undefined,
      `Expected --key value, got ${key ?? "<end>"}`);
    parsed[key.slice(2)] = args[index + 1];
  }
  return parsed;
}

function required(parsed, key) {
  const value = parsed[key];
  assert(value, `Missing --${key}`);
  return value;
}

function resolved(parsed, key) {
  const value = path.resolve(required(parsed, key));
  assert(existsSync(value), `Missing input: ${value}`);
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log(`Usage:
  node ${GENERATOR} build --cohort <schema43.json> --output <receipt.json>
  node ${GENERATOR} verify --input <receipt.json>
  node ${GENERATOR} self-test`);
  process.exit(exitCode);
}
