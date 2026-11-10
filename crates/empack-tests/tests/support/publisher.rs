//! Independent JSON/signature assertions over the product's publisher output.
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

pub fn verify_signed_payload(envelope: &Path, kind: &str, public: &VerifyingKey) -> Vec<u8> {
    let prefix: &[u8] = match kind {
        "release" => b"empack.release.v1\0",
        "channel" => b"empack.channel.v1\0",
        _ => panic!("unsupported fixture envelope kind"),
    };
    let value: serde_json::Value = serde_json::from_slice(&fs::read(envelope).unwrap()).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["schema"], 1);
    assert_eq!(value["kind"], kind);
    let payload = unhex(value["payload"].as_str().unwrap());
    let signatures = value["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 1);
    assert_eq!(signatures[0].as_object().unwrap().len(), 3);
    assert_eq!(signatures[0]["algorithm"], "ed25519");
    assert_eq!(
        signatures[0]["key"],
        hex(&Sha256::digest(public.as_bytes()))
    );
    let signature =
        Signature::from_slice(&unhex(signatures[0]["signature"].as_str().unwrap())).unwrap();
    public
        .verify_strict(&[prefix, &payload].concat(), &signature)
        .unwrap();
    payload
}

pub fn verify_staged(envelope: &Path, public: &VerifyingKey, id: &str, original: &Path) {
    let payload = verify_signed_payload(envelope, "release", public);
    assert_eq!(payload, fs::read(original).unwrap());
    assert_eq!(hex(&Sha256::digest(&payload)), id);
}
