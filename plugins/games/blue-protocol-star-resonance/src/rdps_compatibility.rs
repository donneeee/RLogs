use std::{collections::BTreeSet, sync::OnceLock};

use serde::Deserialize;

use crate::{
    BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID, BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
    BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST, BPSR_COMPATIBILITY_EPOCH_VERSION, MappingProvenance,
    ProtocolPack,
    compatibility_epoch::{BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS, retarget_protocol_pack},
};

const RECEIPT_SCHEMA_VERSION: u16 = 1;
const EQUIVALENCE_SCOPE: &str =
    "same-build-protocol-definition-identical-except-pack-id-target-and-appended-provenance";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SameBuildBootstrapAuthorityReceipt {
    schema_version: u16,
    compatibility_epoch_version: u16,
    deployment_id: String,
    game_build: String,
    source_protocol_pack_digest: String,
    derivation: String,
    derived_target_deployment_id: String,
    equivalence_scope: String,
    variants: Vec<SameBuildBootstrapAuthorityVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SameBuildBootstrapAuthorityVariant {
    channel: String,
    protocol_pack_digest: String,
}

static REVIEWED_DIGESTS: OnceLock<Result<BTreeSet<String>, String>> = OnceLock::new();

pub(crate) fn reviewed_same_build_bootstrap_rdps_digests()
-> Result<&'static BTreeSet<String>, String> {
    REVIEWED_DIGESTS
        .get_or_init(load_reviewed_digests)
        .as_ref()
        .map_err(Clone::clone)
}

fn load_reviewed_digests() -> Result<BTreeSet<String>, String> {
    let receipt: SameBuildBootstrapAuthorityReceipt = serde_json::from_slice(include_bytes!(
        "../game-data/runtime/rdps-same-build-bootstrap-authority.v1.json"
    ))
    .map_err(|error| format!("same-build bootstrap rDPS receipt is invalid: {error}"))?;
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.compatibility_epoch_version != BPSR_COMPATIBILITY_EPOCH_VERSION
        || receipt.deployment_id != BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID
        || receipt.game_build != BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD
        || receipt.source_protocol_pack_digest != BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST
        || receipt.derivation != "client-bootstrap"
        || receipt.derived_target_deployment_id != "unknown"
        || receipt.equivalence_scope != EQUIVALENCE_SCOPE
    {
        return Err(
            "same-build bootstrap rDPS receipt is outside the active compatibility epoch".into(),
        );
    }

    let source = ProtocolPack::from_json(include_bytes!(
        "../protocol-packs/global/steam-24687926/pack.json"
    ))
    .map_err(|error| format!("same-build bootstrap rDPS source pack is invalid: {error}"))?;
    if source.digest() != BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST {
        return Err("same-build bootstrap rDPS source digest changed".into());
    }

    let expected_channels = BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let receipt_channels = receipt
        .variants
        .iter()
        .map(|variant| variant.channel.as_str())
        .collect::<BTreeSet<_>>();
    if receipt.variants.len() != expected_channels.len() || receipt_channels != expected_channels {
        return Err("same-build bootstrap rDPS receipt channel inventory changed".into());
    }

    let mut digests = BTreeSet::new();
    for variant in receipt.variants {
        let derived = retarget_protocol_pack(
            &source,
            &receipt.derivation,
            &receipt.derived_target_deployment_id,
            &variant.channel,
            &receipt.game_build,
        )?;
        if derived.digest() != variant.protocol_pack_digest {
            return Err(format!(
                "same-build bootstrap rDPS digest changed for channel {}",
                variant.channel
            ));
        }
        verify_semantic_equivalence(&source, &derived, &variant.channel)?;
        if !digests.insert(variant.protocol_pack_digest) {
            return Err("same-build bootstrap rDPS receipt contains a duplicate digest".into());
        }
    }
    Ok(digests)
}

fn verify_semantic_equivalence(
    source: &ProtocolPack,
    derived: &ProtocolPack,
    channel: &str,
) -> Result<(), String> {
    let source_definition = source.definition();
    let derived_definition = derived.definition();
    let expected_provenance = MappingProvenance {
        source: "provisional-client-bootstrap".into(),
        reference: format!(
            "pack_build={};client_deployment=unknown;client_channel={channel};client_build={}",
            BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD, BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD
        ),
    };
    if derived_definition.schema_version != source_definition.schema_version
        || derived_definition.acquisition != source_definition.acquisition
        || derived_definition.routes != source_definition.routes
        || derived_definition.pack_id
            != format!("{}-client-bootstrap-{channel}", source_definition.pack_id)
        || derived_definition.target.deployment_id != "unknown"
        || derived_definition.target.region_id.is_some()
        || derived_definition.target.channel != channel
        || derived_definition.target.build_id != source_definition.target.build_id
        || derived_definition.target.executable_version
            != source_definition.target.executable_version
        || derived_definition.provenance.len() != source_definition.provenance.len() + 1
        || !derived_definition
            .provenance
            .starts_with(&source_definition.provenance)
        || derived_definition.provenance.last() != Some(&expected_provenance)
    {
        return Err(format!(
            "same-build bootstrap rDPS semantic equivalence failed for channel {channel}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_reconstructs_every_same_build_bootstrap_digest() {
        let digests = reviewed_same_build_bootstrap_rdps_digests().unwrap();
        assert_eq!(digests.len(), BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS.len());
        assert!(!digests.contains(BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST));
    }
}
