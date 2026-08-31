use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;

#[derive(Debug)]
struct Arguments {
    surface: PathBuf,
    wire_proof: PathBuf,
    decoder: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RpcSurface {
    build_id: String,
    source_identity: SourceIdentity,
    messages: Vec<GeneratedMessage>,
    enums: Vec<GeneratedEnum>,
}

#[derive(Debug, Deserialize)]
struct GeneratedEnum {
    full_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SourceIdentity {
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Deserialize, Serialize)]
struct ArtifactIdentity {
    byte_length: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_version: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct NativeWireProof {
    game_build: String,
    source_identity: NativeWireSourceIdentity,
    messages: Vec<NativeWireMessage>,
}

#[derive(Debug, Deserialize)]
struct NativeWireSourceIdentity {
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
    rpc_surface: ArtifactIdentity,
}

#[derive(Debug, Deserialize)]
struct NativeWireMessage {
    full_name: String,
    state: String,
    fields: Vec<NativeWireField>,
}

#[derive(Debug, Deserialize)]
struct NativeWireField {
    order: usize,
    name: String,
    field_type: String,
    protobuf_tag: Option<u32>,
    proof_state: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GeneratedMessage {
    full_name: String,
    fields: Vec<GeneratedField>,
}

#[derive(Debug, Clone, Deserialize)]
struct GeneratedField {
    order: usize,
    name: String,
    field_type: String,
}

#[derive(Debug, Clone)]
struct DecoderMessage {
    name: String,
    fields: Vec<DecoderField>,
}

#[derive(Debug, Clone)]
struct DecoderField {
    name: String,
    tag: u32,
    prost_shape: String,
    rust_type: String,
}

#[derive(Debug, Serialize)]
struct ContractAudit {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    build_id: String,
    source_identity: SourceIdentity,
    decoder_source: DecoderSource,
    wire_proof_source: DecoderSource,
    policy: Policy,
    summary: Summary,
    message_aliases: Vec<MessageAlias>,
    alias_conflicts: Vec<AliasConflict>,
    messages: Vec<MessageAudit>,
    ambiguous_generated_short_names: Vec<AmbiguousName>,
}

#[derive(Debug, Serialize)]
struct DecoderSource {
    repository_relative_path: String,
    byte_length: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct Policy {
    generated_field_order_treated_as_protobuf_tag: bool,
    exact_native_wire_tag_required: bool,
    missing_generated_fields_hidden: bool,
    missing_fields_auto_added_to_decoder: bool,
    unknown_fields_discarded_from_audit: bool,
    exact_build_packet_replay_required: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    decoder_messages: usize,
    exact_generated_message_matches: usize,
    decoder_messages_missing_from_generated_surface: usize,
    decoder_fields: usize,
    exact_name_matches: usize,
    normalized_name_matches: usize,
    exact_tag_name_mismatch_candidates: usize,
    protobuf_tag_mismatches: usize,
    decoder_only_fields: usize,
    generated_fields_absent_from_decoder: usize,
    shape_mismatch_candidates: usize,
    messages_requiring_review: usize,
    proven_message_aliases: usize,
    alias_conflicts: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MessageAlias {
    decoder_name: String,
    generated_full_name: String,
    evidence_parent_decoder: String,
    evidence_parent_generated: String,
    evidence_decoder_field: String,
    evidence_generated_field: String,
    evidence_state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct AliasConflict {
    decoder_name: String,
    candidate_generated_full_names: Vec<String>,
    evidence_parent_decoder: String,
    evidence_decoder_field: String,
    state: &'static str,
}

#[derive(Debug, Serialize)]
struct MessageAudit {
    decoder_name: String,
    generated_full_name: Option<String>,
    state: &'static str,
    decoder_field_count: usize,
    generated_field_count: usize,
    fields: Vec<FieldAudit>,
    generated_fields_absent_from_decoder: Vec<GeneratedOnlyField>,
}

#[derive(Debug, Serialize)]
struct FieldAudit {
    decoder_name: String,
    decoder_tag: u32,
    prost_shape: String,
    rust_type: String,
    generated_name: Option<String>,
    generated_order: Option<usize>,
    generated_type: Option<String>,
    generated_protobuf_tag: Option<u32>,
    tag_state: &'static str,
    match_state: &'static str,
    shape_state: &'static str,
}

#[derive(Debug, Serialize)]
struct GeneratedOnlyField {
    order: usize,
    name: String,
    field_type: String,
    protobuf_tag: Option<u32>,
    tag_state: &'static str,
}

#[derive(Debug, Serialize)]
struct AmbiguousName {
    short_name: String,
    full_names: Vec<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RPC decoder contract audit failed: {error}");
        std::process::exit(1);
    }
}

fn normalized_sha256(value: &str) -> &str {
    value.strip_prefix("sha256:").unwrap_or(value)
}

fn same_identity(left: &ArtifactIdentity, right: &ArtifactIdentity) -> bool {
    left.byte_length == right.byte_length
        && normalized_sha256(&left.sha256).eq_ignore_ascii_case(normalized_sha256(&right.sha256))
}

fn validate_wire_proof(
    surface: &RpcSurface,
    surface_bytes: &[u8],
    proof: &NativeWireProof,
) -> Result<(), Box<dyn Error>> {
    if proof.game_build != surface.build_id {
        return Err("native wire proof build does not match the RPC surface".into());
    }
    if !same_identity(
        &proof.source_identity.metadata,
        &surface.source_identity.metadata,
    ) || !same_identity(
        &proof.source_identity.game_assembly,
        &surface.source_identity.game_assembly,
    ) {
        return Err("native wire proof binary identity does not match the RPC surface".into());
    }
    let observed_surface_sha = format!("{:x}", Sha256::digest(surface_bytes));
    if proof.source_identity.rpc_surface.byte_length != surface_bytes.len() as u64
        || !normalized_sha256(&proof.source_identity.rpc_surface.sha256)
            .eq_ignore_ascii_case(&observed_surface_sha)
    {
        return Err("native wire proof is not bound to the supplied RPC surface bytes".into());
    }
    if proof.messages.iter().any(|message| {
        message.state != "exact"
            || message.fields.iter().any(|field| {
                field.protobuf_tag.is_none()
                    || (field.proof_state != "exact_native_merge_branch"
                        && field.proof_state != "exact_native_value_struct_processor_branch")
            })
    }) {
        return Err("native wire proof contains unresolved field tags".into());
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(env::args().skip(1))?;
    let surface_bytes = fs::read(&arguments.surface)?;
    let surface: RpcSurface = serde_json::from_slice(&surface_bytes)?;
    let wire_proof_bytes = fs::read(&arguments.wire_proof)?;
    let wire_proof: NativeWireProof = serde_json::from_slice(&wire_proof_bytes)?;
    validate_wire_proof(&surface, &surface_bytes, &wire_proof)?;
    let decoder_bytes = fs::read(&arguments.decoder)?;
    let decoder_text = std::str::from_utf8(&decoder_bytes)?;
    let decoder_messages = parse_decoder(decoder_text)?;

    // Exact value-struct schemas may be absent from the RPC surface because
    // they are serialized by generated processor classes rather than ordinary
    // IMessage classes. The native wire proof is allowed to augment only those
    // missing messages whose every field and tag was proven from the exact
    // build's processor branch and instance offset.
    let mut generated_messages = surface.messages.clone();
    let mut known_generated_names = generated_messages
        .iter()
        .map(|message| message.full_name.clone())
        .collect::<BTreeSet<_>>();
    for native in &wire_proof.messages {
        if known_generated_names.insert(native.full_name.clone()) {
            generated_messages.push(GeneratedMessage {
                full_name: native.full_name.clone(),
                fields: native
                    .fields
                    .iter()
                    .map(|field| GeneratedField {
                        order: field.order,
                        name: field.name.clone(),
                        field_type: field.field_type.clone(),
                    })
                    .collect(),
            });
        }
    }

    let mut generated_by_short_name: BTreeMap<String, Vec<GeneratedMessage>> = BTreeMap::new();
    for message in &generated_messages {
        let short_name = message
            .full_name
            .rsplit('.')
            .next()
            .unwrap_or(&message.full_name)
            .to_owned();
        generated_by_short_name
            .entry(short_name)
            .or_default()
            .push(message.clone());
    }
    let generated_enum_names = surface
        .enums
        .iter()
        .flat_map(|enumeration| {
            [
                enumeration.full_name.clone(),
                enumeration
                    .full_name
                    .rsplit('.')
                    .next()
                    .unwrap_or(&enumeration.full_name)
                    .to_owned(),
            ]
        })
        .collect::<BTreeSet<_>>();
    let native_wire_by_message = wire_proof
        .messages
        .iter()
        .map(|message| (message.full_name.as_str(), message))
        .collect::<BTreeMap<_, _>>();

    let ambiguous_generated_short_names = generated_by_short_name
        .iter()
        .filter(|(_, messages)| messages.len() > 1)
        .map(|(short_name, messages)| AmbiguousName {
            short_name: short_name.clone(),
            full_names: messages
                .iter()
                .map(|message| message.full_name.clone())
                .collect(),
        })
        .collect::<Vec<_>>();

    let (resolved_messages, message_aliases, alias_conflicts) =
        resolve_message_aliases(&decoder_messages, &generated_by_short_name);

    let mut summary = Summary {
        decoder_messages: decoder_messages.len(),
        exact_generated_message_matches: 0,
        decoder_messages_missing_from_generated_surface: 0,
        decoder_fields: 0,
        exact_name_matches: 0,
        normalized_name_matches: 0,
        exact_tag_name_mismatch_candidates: 0,
        protobuf_tag_mismatches: 0,
        decoder_only_fields: 0,
        generated_fields_absent_from_decoder: 0,
        shape_mismatch_candidates: 0,
        messages_requiring_review: 0,
        proven_message_aliases: message_aliases.len(),
        alias_conflicts: alias_conflicts.len(),
    };
    let mut audits = Vec::new();

    for decoder in decoder_messages {
        summary.decoder_fields += decoder.fields.len();
        let generated = resolved_messages
            .get(&decoder.name)
            .and_then(|full_name| find_generated_message(&generated_messages, full_name));
        let Some(generated) = generated else {
            summary.decoder_messages_missing_from_generated_surface += 1;
            summary.decoder_only_fields += decoder.fields.len();
            summary.messages_requiring_review += 1;
            audits.push(MessageAudit {
                decoder_name: decoder.name,
                generated_full_name: None,
                state: "decoder_message_not_uniquely_resolved",
                decoder_field_count: decoder.fields.len(),
                generated_field_count: 0,
                fields: decoder
                    .fields
                    .into_iter()
                    .map(|field| FieldAudit {
                        decoder_name: field.name,
                        decoder_tag: field.tag,
                        prost_shape: field.prost_shape,
                        rust_type: field.rust_type,
                        generated_name: None,
                        generated_order: None,
                        generated_type: None,
                        generated_protobuf_tag: None,
                        tag_state: "not_compared",
                        match_state: "decoder_only",
                        shape_state: "not_compared",
                    })
                    .collect(),
                generated_fields_absent_from_decoder: Vec::new(),
            });
            continue;
        };
        summary.exact_generated_message_matches += 1;
        let native_wire = native_wire_by_message
            .get(generated.full_name.as_str())
            .ok_or_else(|| {
                format!(
                    "native wire proof is missing resolved message {}",
                    generated.full_name
                )
            })?;
        if native_wire.state != "exact"
            || native_wire.fields.iter().any(|field| {
                field.protobuf_tag.is_none()
                    || (field.proof_state != "exact_native_merge_branch"
                        && field.proof_state != "exact_native_value_struct_processor_branch")
            })
        {
            return Err(format!(
                "native wire proof is incomplete for resolved message {}",
                generated.full_name
            )
            .into());
        }
        let mut matched_orders = BTreeSet::new();
        let mut field_audits = Vec::new();
        let mut requires_review = false;

        for decoder_field in &decoder.fields {
            let exact = generated
                .fields
                .iter()
                .find(|field| field.name == decoder_field.name);
            let normalized = generated
                .fields
                .iter()
                .find(|field| normalize_name(&field.name) == normalize_name(&decoder_field.name));
            let native_tag_match = native_wire
                .fields
                .iter()
                .find(|field| field.protobuf_tag == Some(decoder_field.tag));
            let exact_tag_generated = native_tag_match.and_then(|wire_field| {
                generated
                    .fields
                    .iter()
                    .find(|field| field.name == wire_field.name)
            });
            let (matched, match_state) = if let Some(field) = exact {
                summary.exact_name_matches += 1;
                (Some(field), "exact_name")
            } else if let Some(field) = normalized {
                summary.normalized_name_matches += 1;
                (Some(field), "normalized_name")
            } else if let Some(field) = exact_tag_generated {
                summary.exact_tag_name_mismatch_candidates += 1;
                requires_review = true;
                (Some(field), "exact_tag_name_mismatch")
            } else {
                summary.decoder_only_fields += 1;
                requires_review = true;
                (None, "decoder_only")
            };
            let generated_protobuf_tag = matched.and_then(|field| {
                native_wire
                    .fields
                    .iter()
                    .find(|wire_field| wire_field.name == field.name)
                    .and_then(|wire_field| wire_field.protobuf_tag)
            });
            let tag_state = match generated_protobuf_tag {
                Some(tag) if tag == decoder_field.tag => "exact_native_tag_match",
                Some(_) => {
                    summary.protobuf_tag_mismatches += 1;
                    requires_review = true;
                    "exact_native_tag_mismatch"
                }
                None => "not_compared",
            };
            let shape_state = matched
                .map(|field| shape_state(decoder_field, field, &generated_enum_names))
                .unwrap_or("not_compared");
            if shape_state == "candidate_mismatch" {
                summary.shape_mismatch_candidates += 1;
                requires_review = true;
            }
            if let Some(field) = matched {
                matched_orders.insert(field.order);
            }
            field_audits.push(FieldAudit {
                decoder_name: decoder_field.name.clone(),
                decoder_tag: decoder_field.tag,
                prost_shape: decoder_field.prost_shape.clone(),
                rust_type: decoder_field.rust_type.clone(),
                generated_name: matched.map(|field| field.name.clone()),
                generated_order: matched.map(|field| field.order),
                generated_type: matched.map(|field| field.field_type.clone()),
                generated_protobuf_tag,
                tag_state,
                match_state,
                shape_state,
            });
        }

        let generated_fields_absent_from_decoder = generated
            .fields
            .iter()
            .filter(|field| !matched_orders.contains(&field.order))
            .map(|field| GeneratedOnlyField {
                order: field.order,
                name: field.name.clone(),
                field_type: field.field_type.clone(),
                protobuf_tag: native_wire
                    .fields
                    .iter()
                    .find(|wire_field| wire_field.name == field.name)
                    .and_then(|wire_field| wire_field.protobuf_tag),
                tag_state: "exact_native_merge_branch",
            })
            .collect::<Vec<_>>();
        if !generated_fields_absent_from_decoder.is_empty() {
            summary.generated_fields_absent_from_decoder +=
                generated_fields_absent_from_decoder.len();
            requires_review = true;
        }
        if requires_review {
            summary.messages_requiring_review += 1;
        }
        audits.push(MessageAudit {
            decoder_name: decoder.name,
            generated_full_name: Some(generated.full_name.clone()),
            state: if requires_review {
                "review_required"
            } else {
                "current_surface_match"
            },
            decoder_field_count: decoder.fields.len(),
            generated_field_count: generated.fields.len(),
            fields: field_audits,
            generated_fields_absent_from_decoder,
        });
    }

    audits.sort_by(|left, right| left.decoder_name.cmp(&right.decoder_name));
    let report = ContractAudit {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rpc-decoder-contract-audit",
        game: "blue-protocol-star-resonance",
        build_id: surface.build_id,
        source_identity: surface.source_identity,
        decoder_source: DecoderSource {
            repository_relative_path:
                "plugins/games/blue-protocol-star-resonance/src/game_schema_v1.rs".to_owned(),
            byte_length: decoder_bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(&decoder_bytes)),
        },
        wire_proof_source: DecoderSource {
            repository_relative_path: arguments.wire_proof.to_string_lossy().replace('\\', "/"),
            byte_length: wire_proof_bytes.len() as u64,
            sha256: format!("sha256:{:x}", Sha256::digest(&wire_proof_bytes)),
        },
        policy: Policy {
            generated_field_order_treated_as_protobuf_tag: false,
            exact_native_wire_tag_required: true,
            missing_generated_fields_hidden: false,
            missing_fields_auto_added_to_decoder: false,
            unknown_fields_discarded_from_audit: false,
            exact_build_packet_replay_required: true,
        },
        summary,
        message_aliases,
        alias_conflicts,
        messages: audits,
        ambiguous_generated_short_names,
    };
    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&arguments.output, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "audited {} decoder messages; {} require review; wrote {}",
        report.summary.decoder_messages,
        report.summary.messages_requiring_review,
        arguments.output.display()
    );
    Ok(())
}

fn resolve_message_aliases(
    decoder_messages: &[DecoderMessage],
    generated_by_short_name: &BTreeMap<String, Vec<GeneratedMessage>>,
) -> (
    BTreeMap<String, String>,
    Vec<MessageAlias>,
    Vec<AliasConflict>,
) {
    let decoder_by_name = decoder_messages
        .iter()
        .map(|message| (message.name.clone(), message))
        .collect::<BTreeMap<_, _>>();
    let mut resolved = BTreeMap::new();
    for decoder in decoder_messages {
        if let Some(messages) = generated_by_short_name.get(&decoder.name) {
            if messages.len() == 1 {
                resolved.insert(decoder.name.clone(), messages[0].full_name.clone());
            }
        }
    }

    let mut aliases = Vec::new();
    let mut conflicts = Vec::new();
    loop {
        let mut proposals: BTreeMap<String, Vec<(String, MessageAlias)>> = BTreeMap::new();
        for (parent_decoder_name, parent_generated_full_name) in &resolved {
            let Some(parent_decoder) = decoder_by_name.get(parent_decoder_name) else {
                continue;
            };
            let Some(parent_generated) = generated_by_short_name
                .values()
                .flatten()
                .find(|message| message.full_name == *parent_generated_full_name)
            else {
                continue;
            };
            for decoder_field in &parent_decoder.fields {
                let Some((generated_field, evidence_state)) =
                    match_generated_field_by_name(decoder_field, parent_generated)
                else {
                    continue;
                };
                let Some(decoder_type) = referenced_decoder_message_type(&decoder_field.rust_type)
                else {
                    continue;
                };
                if resolved.contains_key(&decoder_type)
                    || !decoder_by_name.contains_key(&decoder_type)
                {
                    continue;
                }
                let Some(generated_type) =
                    referenced_generated_message_type(&generated_field.field_type)
                else {
                    continue;
                };
                let Some(generated_candidates) = generated_by_short_name.get(&generated_type)
                else {
                    continue;
                };
                if generated_candidates.len() != 1 {
                    continue;
                }
                let candidate = generated_candidates[0].full_name.clone();
                proposals.entry(decoder_type.clone()).or_default().push((
                    candidate.clone(),
                    MessageAlias {
                        decoder_name: decoder_type,
                        generated_full_name: candidate,
                        evidence_parent_decoder: parent_decoder.name.clone(),
                        evidence_parent_generated: parent_generated.full_name.clone(),
                        evidence_decoder_field: decoder_field.name.clone(),
                        evidence_generated_field: generated_field.name.clone(),
                        evidence_state,
                    },
                ));
            }
        }

        let mut added = 0usize;
        for (decoder_name, candidates) in proposals {
            let unique = candidates
                .iter()
                .map(|(full_name, _)| full_name.clone())
                .collect::<BTreeSet<_>>();
            if unique.len() == 1 {
                let evidence = candidates[0].1.clone();
                resolved.insert(decoder_name, evidence.generated_full_name.clone());
                aliases.push(evidence);
                added += 1;
            } else if unique.len() > 1
                && !conflicts
                    .iter()
                    .any(|conflict: &AliasConflict| conflict.decoder_name == decoder_name)
            {
                conflicts.push(AliasConflict {
                    decoder_name,
                    candidate_generated_full_names: unique.into_iter().collect(),
                    evidence_parent_decoder: candidates[0].1.evidence_parent_decoder.clone(),
                    evidence_decoder_field: candidates[0].1.evidence_decoder_field.clone(),
                    state: "conflicting_parent_field_evidence",
                });
            }
        }
        if added == 0 {
            break;
        }
    }
    aliases.sort_by(|left, right| left.decoder_name.cmp(&right.decoder_name));
    conflicts.sort_by(|left, right| left.decoder_name.cmp(&right.decoder_name));
    (resolved, aliases, conflicts)
}

fn find_generated_message<'a>(
    messages: &'a [GeneratedMessage],
    full_name: &str,
) -> Option<&'a GeneratedMessage> {
    messages
        .iter()
        .find(|message| message.full_name == full_name)
}

fn match_generated_field_by_name<'a>(
    decoder: &DecoderField,
    generated: &'a GeneratedMessage,
) -> Option<(&'a GeneratedField, &'static str)> {
    let normalized = normalize_name(&decoder.name);
    let exact = generated
        .fields
        .iter()
        .filter(|field| normalize_name(&field.name) == normalized)
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return Some((exact[0], "normalized_parent_field_name"));
    }
    let singular = singular_name(&normalized);
    let plural_equivalent = generated
        .fields
        .iter()
        .filter(|field| singular_name(&normalize_name(&field.name)) == singular)
        .collect::<Vec<_>>();
    if plural_equivalent.len() == 1 {
        Some((plural_equivalent[0], "singular_plural_parent_field_name"))
    } else {
        None
    }
}

fn singular_name(value: &str) -> &str {
    value.strip_suffix('s').unwrap_or(value)
}

fn referenced_decoder_message_type(value: &str) -> Option<String> {
    referenced_message_type(value, &["Option", "Vec", "HashMap", "BTreeMap"])
}

fn referenced_generated_message_type(value: &str) -> Option<String> {
    referenced_message_type(value, &["RepeatedField", "MapField"])
}

fn referenced_message_type(value: &str, wrappers: &[&str]) -> Option<String> {
    let excluded = [
        "std",
        "collections",
        "Option",
        "Vec",
        "HashMap",
        "BTreeMap",
        "RepeatedField",
        "MapField",
        "ByteString",
        "String",
        "str",
        "bool",
        "int",
        "uint",
        "long",
        "ulong",
        "float",
        "double",
        "i8",
        "i16",
        "i32",
        "i64",
        "u8",
        "u16",
        "u32",
        "u64",
        "f32",
        "f64",
    ];
    let identifiers = value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
        .filter(|identifier| !excluded.contains(identifier) && !wrappers.contains(identifier))
        .filter(|identifier| !identifier.starts_with('E'))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    (identifiers.len() == 1).then(|| identifiers.into_iter().next().unwrap())
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> Result<Arguments, Box<dyn Error>> {
    let mut values = BTreeMap::new();
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        let key = argument
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected positional argument {argument}"))?;
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for --{key}"))?;
        values.insert(key.to_owned(), value);
    }
    let required = |key: &str| -> Result<PathBuf, Box<dyn Error>> {
        values
            .get(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("missing --{key}").into())
    };
    Ok(Arguments {
        surface: required("surface")?,
        wire_proof: required("wire-proof")?,
        decoder: required("decoder")?,
        output: required("output")?,
    })
}

fn parse_decoder(source: &str) -> Result<Vec<DecoderMessage>, Box<dyn Error>> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut messages = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index].trim();
        let name = line
            .strip_prefix("pub(crate) struct ")
            .or_else(|| line.strip_prefix("pub struct "))
            .and_then(|value| value.split_whitespace().next())
            .map(|value| value.trim_end_matches('{').to_owned());
        let Some(name) = name else {
            index += 1;
            continue;
        };
        let mut depth = brace_delta(line);
        let mut fields = Vec::new();
        let mut pending_prost = None;
        index += 1;
        while index < lines.len() {
            let line = lines[index].trim();
            if line.starts_with("#[prost(") {
                pending_prost = Some(line.to_owned());
            } else if let Some(declaration) =
                line.strip_prefix("pub ").filter(|line| line.ends_with(','))
            {
                if let Some(prost) = pending_prost.take() {
                    let (field_name, rust_type) = declaration
                        .trim_end_matches(',')
                        .split_once(':')
                        .ok_or_else(|| format!("invalid decoder field {declaration}"))?;
                    fields.push(DecoderField {
                        name: field_name.trim().to_owned(),
                        tag: parse_prost_tag(&prost)?,
                        prost_shape: parse_prost_shape(&prost),
                        rust_type: rust_type.trim().to_owned(),
                    });
                }
            }
            depth += brace_delta(line);
            index += 1;
            if depth == 0 {
                break;
            }
        }
        if !fields.is_empty() {
            messages.push(DecoderMessage { name, fields });
        }
    }
    Ok(messages)
}

fn parse_prost_tag(attribute: &str) -> Result<u32, Box<dyn Error>> {
    let value = attribute
        .split("tag = \"")
        .nth(1)
        .and_then(|value| value.split('"').next())
        .ok_or_else(|| format!("prost attribute has no tag: {attribute}"))?;
    Ok(value.parse()?)
}

fn parse_prost_shape(attribute: &str) -> String {
    attribute
        .trim_start_matches("#[prost(")
        .trim_end_matches(")]")
        .split(", tag =")
        .next()
        .unwrap_or(attribute)
        .to_owned()
}

fn brace_delta(line: &str) -> i32 {
    line.bytes().fold(0, |depth, byte| match byte {
        b'{' => depth + 1,
        b'}' => depth - 1,
        _ => depth,
    })
}

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn shape_state(
    decoder: &DecoderField,
    generated: &GeneratedField,
    generated_enum_names: &BTreeSet<String>,
) -> &'static str {
    let generated_shape = generated_shape(&generated.field_type, generated_enum_names);
    let decoder_shape = decoder_shape(&decoder.prost_shape, &decoder.rust_type);
    if generated_shape == decoder_shape || generated_shape == "message" && decoder_shape == "bytes"
    {
        "compatible_or_intentionally_raw"
    } else {
        "candidate_mismatch"
    }
}

fn generated_shape(value: &str, generated_enum_names: &BTreeSet<String>) -> &'static str {
    if value.starts_with("RepeatedField<") {
        "repeated"
    } else if value.starts_with("MapField<") {
        "map"
    } else if generated_enum_names.contains(value) {
        "integer"
    } else {
        match value {
            "int" | "uint" | "long" | "ulong" => "integer",
            "bool" => "bool",
            "float" | "double" => "float",
            "string" => "string",
            "ByteString" => "bytes",
            _ => "message",
        }
    }
}

fn decoder_shape(prost: &str, rust_type: &str) -> &'static str {
    if prost.contains("map =") {
        "map"
    } else if prost.contains("repeated") {
        "repeated"
    } else if prost.contains("bytes") {
        "bytes"
    } else if prost.contains("message") {
        "message"
    } else if prost.contains("bool") {
        "bool"
    } else if prost.contains("float") || prost.contains("double") {
        "float"
    } else if prost.contains("string") {
        "string"
    } else if prost.contains("int") || prost.contains("fixed") || rust_type.contains("i64") {
        "integer"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prost_messages_and_tags() {
        let source = "#[derive(Clone, PartialEq, Message)]\npub(crate) struct Demo {\n#[prost(int64, optional, tag = \"1\")]\npub actor_uuid: Option<i64>,\n#[prost(message, repeated, tag = \"4\")]\npub effects: Vec<Effect>,\n}\n";
        let messages = parse_decoder(source).expect("decoder");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].fields[1].tag, 4);
        assert_eq!(messages[0].fields[1].prost_shape, "message, repeated");
    }

    #[test]
    fn normalizes_snake_and_pascal_names() {
        assert_eq!(
            normalize_name("slot_skill_info_map"),
            normalize_name("SlotSkillInfoMap")
        );
    }

    #[test]
    fn treats_int_widths_as_same_wire_family_candidate() {
        let decoder = DecoderField {
            name: "value".to_owned(),
            tag: 1,
            prost_shape: "int64, optional".to_owned(),
            rust_type: "Option<i64>".to_owned(),
        };
        let generated = GeneratedField {
            order: 1,
            name: "Value".to_owned(),
            field_type: "long".to_owned(),
        };
        assert_eq!(
            shape_state(&decoder, &generated, &BTreeSet::new()),
            "compatible_or_intentionally_raw"
        );
    }

    #[test]
    fn resolves_nested_message_alias_from_matched_parent_field() {
        let decoder = vec![
            DecoderMessage {
                name: "SkillEffect".to_owned(),
                fields: vec![DecoderField {
                    name: "damage".to_owned(),
                    tag: 2,
                    prost_shape: "message, repeated".to_owned(),
                    rust_type: "Vec<DamageInfo>".to_owned(),
                }],
            },
            DecoderMessage {
                name: "DamageInfo".to_owned(),
                fields: vec![DecoderField {
                    name: "value".to_owned(),
                    tag: 6,
                    prost_shape: "int64, optional".to_owned(),
                    rust_type: "Option<i64>".to_owned(),
                }],
            },
        ];
        let mut generated = BTreeMap::new();
        generated.insert(
            "SkillEffect".to_owned(),
            vec![GeneratedMessage {
                full_name: "Zproto.SkillEffect".to_owned(),
                fields: vec![GeneratedField {
                    order: 2,
                    name: "Damages".to_owned(),
                    field_type: "RepeatedField<SyncDamageInfo>".to_owned(),
                }],
            }],
        );
        generated.insert(
            "SyncDamageInfo".to_owned(),
            vec![GeneratedMessage {
                full_name: "Zproto.SyncDamageInfo".to_owned(),
                fields: vec![GeneratedField {
                    order: 6,
                    name: "Value".to_owned(),
                    field_type: "long".to_owned(),
                }],
            }],
        );

        let (resolved, aliases, conflicts) = resolve_message_aliases(&decoder, &generated);
        assert_eq!(resolved["DamageInfo"], "Zproto.SyncDamageInfo");
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].evidence_decoder_field, "damage");
        assert!(conflicts.is_empty());
    }
}
