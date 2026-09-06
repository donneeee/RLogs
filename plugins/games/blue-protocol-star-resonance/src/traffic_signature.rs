use rlogs_capture::TcpPayloadDirection;

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
    (matches_scene_server_payload(payload) || matches_login_return_payload(payload))
        .then_some(TcpPayloadDirection::ServerToClient)
}

fn matches_scene_server_payload(payload: &[u8]) -> bool {
    if payload.len() <= 10 || payload[4] != 0 {
        return false;
    }
    let mut data = &payload[10..];
    while data.len() >= 4 {
        let packet_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if packet_length < 4 || packet_length > data.len() {
            return false;
        }
        let packet = &data[4..packet_length];
        if packet.len() >= 5 + SCENE_SERVER_SIGNATURE.len()
            && packet[5..5 + SCENE_SERVER_SIGNATURE.len()] == SCENE_SERVER_SIGNATURE
        {
            return true;
        }
        data = &data[packet_length..];
    }
    false
}

fn matches_login_return_payload(payload: &[u8]) -> bool {
    payload.len() == 0x62
        && payload[..LOGIN_RETURN_PREFIX.len()] == LOGIN_RETURN_PREFIX
        && payload[14..14 + LOGIN_RETURN_BODY_PREFIX.len()] == LOGIN_RETURN_BODY_PREFIX
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
        payload[19] ^= 1;
        assert_eq!(classify_bpsr_tcp_payload(&payload), None);
    }

    #[test]
    fn generic_framed_tcp_payload_is_not_enough_to_claim_game_traffic() {
        let mut payload = vec![0_u8; 10];
        payload[4] = 0;
        payload.extend_from_slice(&12_u32.to_be_bytes());
        payload.extend_from_slice(&[0_u8; 8]);

        assert_eq!(classify_bpsr_tcp_payload(&payload), None);
    }
}
