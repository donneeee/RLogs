use crate::{MappingProvenance, ProtocolPack};

pub const BPSR_COMPATIBILITY_EPOCH_VERSION: u16 = 1;
pub const BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID: &str = "global";
pub const BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD: &str = "24687926";
pub const BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST: &str =
    "sha256:4372050d9d549808b229b16de315080f9bac427efe9602dabd9b93c4502dbbae";

pub(crate) const BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS: &[&str] = &[
    "standalone",
    "epic",
    "starsea",
    "starasia",
    "starsea-steam",
    "starasia-steam",
    "starttw",
    "star",
    "unknown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpsrRuntimeAuthority {
    ExactReviewed,
    CompatibilityEpoch { version: u16 },
}

impl BpsrRuntimeAuthority {
    pub fn is_exact(self) -> bool {
        matches!(self, Self::ExactReviewed)
    }
}

pub(crate) fn retarget_protocol_pack(
    pack: &ProtocolPack,
    derivation: &str,
    deployment_id: &str,
    channel: &str,
    build_id: &str,
) -> Result<ProtocolPack, String> {
    let source_build = pack.definition().target.build_id.clone();
    let mut definition = pack.definition().clone();
    definition.pack_id = format!("{}-{derivation}-{channel}", definition.pack_id);
    definition.target.deployment_id = deployment_id.to_owned();
    definition.target.region_id = None;
    definition.target.channel = channel.to_owned();
    definition.target.build_id = build_id.to_owned();
    definition.provenance.push(MappingProvenance {
        source: format!("provisional-{derivation}"),
        reference: format!(
            "pack_build={source_build};client_deployment={deployment_id};client_channel={channel};client_build={build_id}"
        ),
    });
    ProtocolPack::build(definition).map_err(|error| error.to_string())
}

fn reviewed_epoch_pack() -> Result<ProtocolPack, String> {
    let pack = ProtocolPack::from_json(include_bytes!(
        "../protocol-packs/global/steam-24687926/pack.json"
    ))
    .map_err(|error| format!("bundled BPSR compatibility-epoch pack is invalid: {error}"))?;
    if pack.definition().target.deployment_id != BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID
        || pack.definition().target.channel != "steam"
        || pack.definition().target.build_id != BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD
        || pack.digest() != BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST
    {
        return Err(
            "bundled BPSR compatibility-epoch identity does not match its reviewed pack".into(),
        );
    }
    Ok(pack)
}

/// Resolves exact or explicitly carried-forward runtime authority by rebuilding
/// the reviewed derived pack and comparing its content digest. A build label by
/// itself never grants semantic authority.
pub fn bpsr_runtime_authority(
    deployment_id: &str,
    client_build: &str,
    protocol_pack_digest: &str,
) -> Result<Option<BpsrRuntimeAuthority>, String> {
    let pack = reviewed_epoch_pack()?;
    if deployment_id == BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID
        && client_build == BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD
        && protocol_pack_digest == BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST
    {
        return Ok(Some(BpsrRuntimeAuthority::ExactReviewed));
    }
    if client_build.is_empty() || !client_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }

    if deployment_id == BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID {
        let fallback = retarget_protocol_pack(
            &pack,
            "compatibility-fallback",
            BPSR_COMPATIBILITY_EPOCH_DEPLOYMENT_ID,
            "steam",
            client_build,
        )?;
        if fallback.digest() == protocol_pack_digest {
            return Ok(Some(BpsrRuntimeAuthority::CompatibilityEpoch {
                version: BPSR_COMPATIBILITY_EPOCH_VERSION,
            }));
        }
    }

    if client_build == BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD
        && matches!(deployment_id, "global" | "unknown")
    {
        for channel in BPSR_COMPATIBILITY_BOOTSTRAP_CHANNELS {
            let bootstrap = retarget_protocol_pack(
                &pack,
                "client-bootstrap",
                "unknown",
                channel,
                BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
            )?;
            if bootstrap.digest() == protocol_pack_digest {
                return Ok(Some(BpsrRuntimeAuthority::CompatibilityEpoch {
                    version: BPSR_COMPATIBILITY_EPOCH_VERSION,
                }));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_digest_verified_carry_forward_identities_are_authorized() {
        assert_eq!(
            bpsr_runtime_authority(
                "global",
                BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
                BPSR_COMPATIBILITY_EPOCH_SOURCE_DIGEST,
            )
            .unwrap(),
            Some(BpsrRuntimeAuthority::ExactReviewed),
        );
        let pack = reviewed_epoch_pack().unwrap();
        let steam = retarget_protocol_pack(
            &pack,
            "compatibility-fallback",
            "global",
            "steam",
            "24699999",
        )
        .unwrap();
        assert!(matches!(
            bpsr_runtime_authority("global", "24699999", steam.digest()).unwrap(),
            Some(BpsrRuntimeAuthority::CompatibilityEpoch { version: 1 })
        ));
        assert!(
            crate::bundled_localization_supports_identity("global", "24699999", steam.digest(),)
                .unwrap()
        );
        let bootstrap = retarget_protocol_pack(
            &pack,
            "client-bootstrap",
            "unknown",
            "standalone",
            BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
        )
        .unwrap();
        assert!(matches!(
            bpsr_runtime_authority(
                "global",
                BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
                bootstrap.digest(),
            )
            .unwrap(),
            Some(BpsrRuntimeAuthority::CompatibilityEpoch { version: 1 })
        ));
        assert!(
            crate::bundled_localization_supports_identity(
                "global",
                BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
                bootstrap.digest(),
            )
            .unwrap()
        );
        assert!(
            !crate::bundled_localization_supports_identity(
                "unknown",
                BPSR_COMPATIBILITY_EPOCH_SOURCE_BUILD,
                bootstrap.digest(),
            )
            .unwrap(),
            "bootstrap run authority may initialize before world entry, but presentation waits for Global resolution",
        );
    }

    #[test]
    fn carry_forward_rejects_wrong_digest_deployment_and_epoch_source() {
        let pack = reviewed_epoch_pack().unwrap();
        let steam = retarget_protocol_pack(
            &pack,
            "compatibility-fallback",
            "global",
            "steam",
            "24699999",
        )
        .unwrap();
        assert_eq!(
            bpsr_runtime_authority("global", "24699999", "sha256:wrong").unwrap(),
            None
        );
        assert_eq!(
            bpsr_runtime_authority("cn", "24699999", steam.digest()).unwrap(),
            None
        );
        assert_eq!(
            bpsr_runtime_authority("global", "24700000", steam.digest()).unwrap(),
            None,
            "a digest derived for the prior epoch/build tuple cannot cross a rollover",
        );
    }
}
