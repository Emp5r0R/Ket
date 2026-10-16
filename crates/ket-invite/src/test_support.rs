//! Builders for invites that are signed but deliberately malformed.

use invite_capsule::{InviteProfile, NodePublicKey, NodeSigningKey, cbor, signature_input};

use crate::{ControlLocator, KetInvite};

pub(crate) const CODE: &str = "Abcdefghijklmnopqrstuvwxyz012345";
const PAYLOAD_FIELDS: usize = 10;

/// Signs arbitrary payload bytes so tests can reach semantic checks that sit
/// behind a valid signature.
pub(crate) fn sign_raw_payload(payload: &[u8], signing_key: &NodeSigningKey) -> Vec<u8> {
    let signature = signing_key.sign(&signature_input(
        KetInvite::SIGNATURE_DOMAIN,
        KetInvite::APPLICATION_ID,
        payload,
    ));
    let mut output = Vec::new();
    cbor::encode_array(&mut output, 2);
    cbor::encode_bytes(&mut output, payload);
    cbor::encode_bytes(&mut output, &signature);
    output
}

/// Field-by-field payload builder; each override replaces one field's encoded
/// bytes.
pub(crate) struct RawPayload {
    fields: Vec<Vec<u8>>,
}

impl RawPayload {
    pub(crate) fn valid(public_key: &NodePublicKey) -> Self {
        let mut fields = vec![Vec::new(); PAYLOAD_FIELDS];
        cbor::encode_uint(&mut fields[0], 1);
        cbor::encode_text(&mut fields[1], KetInvite::APPLICATION_ID);
        cbor::encode_uint(&mut fields[2], KetInvite::CAPABILITY_TYPE);
        cbor::encode_bytes(&mut fields[3], public_key.as_bytes());
        cbor::encode_array(&mut fields[4], 1);
        ControlLocator::https("ket.example.com", 443)
            .unwrap()
            .encode(&mut fields[4]);
        cbor::encode_text(&mut fields[5], CODE);
        cbor::encode_uint(&mut fields[6], 1);
        cbor::encode_uint(&mut fields[7], 1);
        cbor::encode_uint(&mut fields[8], 0);
        cbor::encode_uint(&mut fields[9], 2_000_000_000);
        Self { fields }
    }

    pub(crate) fn with(mut self, index: usize, encode: impl FnOnce(&mut Vec<u8>)) -> Self {
        self.fields[index].clear();
        encode(&mut self.fields[index]);
        self
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut output = Vec::new();
        cbor::encode_array(&mut output, PAYLOAD_FIELDS);
        for field in &self.fields {
            output.extend_from_slice(field);
        }
        output
    }
}
