use std::collections::BTreeMap;

use rlogs_profiles::{LocalProfilePackage, ProfilePackageSource};
use rlogs_submission::{WebsitePayloadEnvelope, WebsitePayloadRequest};
use serde_json::json;

fn main() {
    let payload = WebsitePayloadEnvelope::new(
        "app.rlogs.game.blue-protocol-star-resonance",
        "character-profile",
        "app.rlogs.bpsr.character-profile",
        1,
        BTreeMap::from([
            ("character-id".into(), "3296036".into()),
            ("deployment".into(), "global".into()),
            ("region".into(), "north-america".into()),
        ]),
        json!({"character":{"character_id":"3296036","region":{"deployment_id":"global","realm_id":null,"region_id":"north-america","world_id":null}},"display_name":"MarieRose"}),
    )
    .unwrap();
    let request = WebsitePayloadRequest::new(
        "/v1/games/blue-protocol-star-resonance/profiles",
        payload,
    )
    .unwrap();
    let source = ProfilePackageSource {
        session_id: "session-one".into(),
        client_build: "24687926".into(),
        protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
        canonical_content_sha256: format!("sha256:{}", "b".repeat(64)),
        observation_count: 2,
        last_event_sequence: 3,
        live_capture: None,
    };
    let mut package = LocalProfilePackage::new(100, source, request).unwrap();
    package.bind_live_capture("dev_device", "rld_device-secret").unwrap();
    println!("{}", serde_json::to_string(&package).unwrap());
}
