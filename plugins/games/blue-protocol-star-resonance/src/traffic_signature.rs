use rlogs_capture::{
    MAX_TCP_SIGNATURE_PREFIX_BYTES, TcpPayloadDirection, TcpPayloadSignatureResult,
};

const SCENE_SERVER_SIGNATURE: [u8; 6] = [0x00, 0x63, 0x33, 0x53, 0x42, 0x00];
const LOGIN_RETURN_PREFIX: [u8; 10] = [0x00, 0x00, 0x00, 0x62, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01];
const LOGIN_RETURN_BODY_PREFIX: [u8; 6] = [0x00, 0x00, 0x00, 0x00, 0x0a, 0x4e];

/// Recognizes the exact early server payloads used to select a BPSR TCP flow.
///
/// This signature is shared by the established StarResonanceDps V1/V2 flow
/// detector and is intentionally narrower than ordinary protobuf framing. A
/// match proves only that the four-tuple carries BPSR protocol traffic. It
/// does not establish launcher, deployment, region, or exact client build.
pub fn classify_bpsr_tcp_payload(payload: &[u8]) -> Option<TcpPayloadDirection> {
    match classify_bpsr_tcp_prefix(payload) {
        TcpPayloadSignatureResult::Match(direction) => Some(direction),
        TcpPayloadSignatureResult::NeedMore | TcpPayloadSignatureResult::Reject => None,
    }
}

/// Incrementally recognizes an exact BPSR server payload from a contiguous
/// TCP prefix. Truncated candidates remain private and request more bytes;
/// structurally invalid candidates are rejected without confirming the flow.
pub fn classify_bpsr_tcp_prefix(payload: &[u8]) -> TcpPayloadSignatureResult {
    if payload.len() > MAX_TCP_SIGNATURE_PREFIX_BYTES {
        return TcpPayloadSignatureResult::Reject;
    }
    if payload.len() >= LOGIN_RETURN_PREFIX.len()
        && payload[..LOGIN_RETURN_PREFIX.len()] == LOGIN_RETURN_PREFIX
    {
        return matches_login_return_prefix(payload);
    }
    let scene = matches_scene_server_prefix(payload);
    let login = matches_login_return_prefix(payload);
    if matches!(scene, TcpPayloadSignatureResult::Match(_)) {
        scene
    } else if matches!(login, TcpPayloadSignatureResult::Match(_)) {
        login
    } else if matches!(scene, TcpPayloadSignatureResult::NeedMore)
        || matches!(login, TcpPayloadSignatureResult::NeedMore)
    {
        TcpPayloadSignatureResult::NeedMore
    } else {
        TcpPayloadSignatureResult::Reject
    }
}

fn matches_scene_server_prefix(payload: &[u8]) -> TcpPayloadSignatureResult {
    if payload.len() < 5 {
        return TcpPayloadSignatureResult::NeedMore;
    }
    if payload[4] != 0 {
        return TcpPayloadSignatureResult::Reject;
    }
    if payload.len() < 10 {
        return TcpPayloadSignatureResult::NeedMore;
    }

    let mut data = &payload[10..];
    loop {
        if data.len() < 4 {
            return if payload.len() == MAX_TCP_SIGNATURE_PREFIX_BYTES {
                TcpPayloadSignatureResult::Reject
            } else {
                TcpPayloadSignatureResult::NeedMore
            };
        }
        let packet_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if packet_length < 4 {
            return TcpPayloadSignatureResult::Reject;
        }
        if packet_length > MAX_TCP_SIGNATURE_PREFIX_BYTES.saturating_sub(10) {
            return TcpPayloadSignatureResult::Reject;
        }
        if packet_length > data.len() {
            return TcpPayloadSignatureResult::NeedMore;
        }
        let packet = &data[4..packet_length];
        if packet.len() >= 5 + SCENE_SERVER_SIGNATURE.len()
            && packet[5..5 + SCENE_SERVER_SIGNATURE.len()] == SCENE_SERVER_SIGNATURE
        {
            return TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient);
        }
        data = &data[packet_length..];
    }
}

fn matches_login_return_prefix(payload: &[u8]) -> TcpPayloadSignatureResult {
    for (index, expected) in LOGIN_RETURN_PREFIX.iter().copied().enumerate() {
        if let Some(actual) = payload.get(index) {
            if *actual != expected {
                return TcpPayloadSignatureResult::Reject;
            }
        } else {
            return TcpPayloadSignatureResult::NeedMore;
        }
    }
    for (offset, expected) in LOGIN_RETURN_BODY_PREFIX.iter().copied().enumerate() {
        let index = 14 + offset;
        if let Some(actual) = payload.get(index) {
            if *actual != expected {
                return TcpPayloadSignatureResult::Reject;
            }
        } else {
            return TcpPayloadSignatureResult::NeedMore;
        }
    }
    if payload.len() < 0x62 {
        TcpPayloadSignatureResult::NeedMore
    } else {
        TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn established_scene_signature_is_detected_at_the_exact_nested_offset() {
        let mut payload = vec![0_u8; 10];
        payload[4] = 0;
        let mut nested = vec![0_u8; 5];
        nested.extend_from_slice(&SCENE_SERVER_SIGNATURE);
        nested.extend_from_slice(&[1, 2, 3]);
        payload.extend_from_slice(&((nested.len() + 4) as u32).to_be_bytes());
        payload.extend_from_slice(&nested);

        assert_eq!(
            classify_bpsr_tcp_payload(&payload),
            Some(TcpPayloadDirection::ServerToClient)
        );
        let mut coalesced = payload.clone();
        coalesced.extend_from_slice(b"next record");
        assert_eq!(
            classify_bpsr_tcp_prefix(&coalesced),
            TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient)
        );
        for split in 0..payload.len() {
            assert_eq!(
                classify_bpsr_tcp_prefix(&payload[..split]),
                TcpPayloadSignatureResult::NeedMore,
                "split at byte {split}"
            );
        }
    }

    #[test]
    fn established_login_return_signature_allows_only_the_known_variable_field() {
        let mut payload = vec![0_u8; 0x62];
        payload[..LOGIN_RETURN_PREFIX.len()].copy_from_slice(&LOGIN_RETURN_PREFIX);
        payload[10..14].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        payload[14..20].copy_from_slice(&LOGIN_RETURN_BODY_PREFIX);

        assert_eq!(
            classify_bpsr_tcp_payload(&payload),
            Some(TcpPayloadDirection::ServerToClient)
        );
        for split in 0..payload.len() {
            assert_eq!(
                classify_bpsr_tcp_prefix(&payload[..split]),
                TcpPayloadSignatureResult::NeedMore,
                "split at byte {split}"
            );
        }
        let mut coalesced = payload.clone();
        coalesced.extend_from_slice(b"next record");
        assert_eq!(
            classify_bpsr_tcp_prefix(&coalesced),
            TcpPayloadSignatureResult::Match(TcpPayloadDirection::ServerToClient)
        );
        payload[19] ^= 1;
        assert_eq!(classify_bpsr_tcp_payload(&payload), None);
        assert_eq!(
            classify_bpsr_tcp_prefix(&payload),
            TcpPayloadSignatureResult::Reject
        );
    }

    #[test]
    fn generic_framed_tcp_payload_is_not_enough_to_claim_game_traffic() {
        let mut payload = vec![0_u8; 10];
        payload[4] = 0;
        payload.extend_from_slice(&12_u32.to_be_bytes());
        payload.extend_from_slice(&[0_u8; 8]);

        assert_eq!(classify_bpsr_tcp_payload(&payload), None);
    }

    #[test]
    fn structurally_invalid_complete_prefix_is_rejected() {
        let mut payload = vec![0_u8; 10];
        payload[4] = 1;
        payload.extend_from_slice(&15_u32.to_be_bytes());
        payload.extend_from_slice(&[0_u8; 11]);

        assert_eq!(
            classify_bpsr_tcp_prefix(&payload),
            TcpPayloadSignatureResult::Reject
        );

        let mut impossible_length = vec![0_u8; 10];
        impossible_length[4] = 0;
        impossible_length.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            classify_bpsr_tcp_prefix(&impossible_length),
            TcpPayloadSignatureResult::Reject
        );

        let mut exact_cap = vec![0_u8; MAX_TCP_SIGNATURE_PREFIX_BYTES];
        exact_cap[4] = 0;
        exact_cap[10..14].copy_from_slice(
            &u32::try_from(MAX_TCP_SIGNATURE_PREFIX_BYTES - 10)
                .unwrap()
                .to_be_bytes(),
        );
        assert_eq!(
            classify_bpsr_tcp_prefix(&exact_cap),
            TcpPayloadSignatureResult::Reject
        );
        exact_cap.push(0);
        assert_eq!(
            classify_bpsr_tcp_prefix(&exact_cap),
            TcpPayloadSignatureResult::Reject
        );
    }
}
