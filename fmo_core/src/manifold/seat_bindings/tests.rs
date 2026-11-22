use bitcoin::secp256k1::{Keypair, SecretKey};
use serde_json::json;

use super::*;

/// fedibtc/manifold `crates/domain/conformance/fman-seat-bindings-v1.json`:
/// shape and canonical-form vectors (their signatures are placeholders)
const CONFORMANCE: &str = include_str!("../fixtures/fman-seat-bindings-v1.json");

fn secret(byte: u8) -> SecretKey {
    SecretKey::from_slice(&[byte; 32]).unwrap()
}

fn compressed(byte: u8) -> String {
    PublicKey::from_secret_key(&Secp256k1::new(), &secret(byte)).to_string()
}

/// A four-guardian config whose broadcast keys are the keys of secrets 1..=4
fn config(meta: &[(&str, &str)]) -> ClientConfig {
    let mut api_endpoints = Map::new();
    let mut broadcast = Map::new();
    for peer in 0..4u8 {
        api_endpoints.insert(
            peer.to_string(),
            json!({ "url": format!("wss://guardian-{peer}.example/"), "name": format!("Guardian {peer}") }),
        );
        broadcast.insert(peer.to_string(), json!(compressed(peer + 1)));
    }
    serde_json::from_value(json!({
        "api_endpoints": api_endpoints,
        "broadcast_public_keys": broadcast,
        "consensus_version": { "major": 2, "minor": 0 },
        "meta": meta.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
        "modules": {},
    }))
    .unwrap()
}

/// The MG for `peer` signs with secret 100 + mg; fee keys come from 200 + peer
fn attestation(seats: &FederationSeats, peer: u16, mg: u8) -> Value {
    let mg_key = Keypair::from_secret_key(&Secp256k1::new(), &secret(100 + mg));
    json!({
        "federation_config_hash": seats.config_hash.to_vec(),
        "federation_id": seats.federation_id,
        "fman_pubkey": mg_key.x_only_public_key().0.to_string(),
        "guardian_fee_account": {
            "acc_type": "BtcDepositor",
            "pub_keys": [compressed(200 + peer as u8)],
            "threshold": 1,
        },
        "guardian_identity": seats.seats[&peer],
        "issued_at": 1_700_000_000u64,
        "peer_id": peer.to_string(),
    })
}

fn sign(attestation: Value, mg: u8) -> Value {
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, &secret(100 + mg));
    let digest = attestation_digest(&attestation).unwrap();
    let signature = secp.sign_schnorr_no_aux_rand(&Message::from_digest(digest), &keypair);
    json!({
        "attestation": attestation,
        "proof": { "signature": URL_SAFE_NO_PAD.encode(signature.as_ref()) },
        "version": 1,
    })
}

fn directory(bindings: Vec<Value>) -> String {
    canonical_json(&json!({ "seat_bindings": bindings, "version": 1 })).unwrap()
}

/// Every seat bound, peers 0 and 1 by the same MG
fn valid_bindings(seats: &FederationSeats) -> Vec<Value> {
    [(0, 0), (1, 0), (2, 1), (3, 2)]
        .into_iter()
        .map(|(peer, mg)| sign(attestation(seats, peer, mg), mg))
        .collect()
}

#[test]
fn conformance_vectors_parse_in_their_peer_order() {
    let conformance: Value = serde_json::from_str(CONFORMANCE).unwrap();
    assert_eq!(conformance["meta_field_key"], SEAT_BINDINGS_META_KEY);
    assert_eq!(conformance["max_seat_bindings"], MAX_SEAT_BINDINGS);
    assert_eq!(conformance["max_value_bytes"], MAX_VALUE_BYTES);
    for vector in conformance["vectors"].as_array().unwrap() {
        let value = vector["canonical_value"].as_str().unwrap();
        let bindings =
            parse_canonical(value).unwrap_or_else(|error| panic!("{}: {error}", vector["name"]));
        let order: Vec<_> = bindings
            .iter()
            .map(|b| b["attestation"]["peer_id"].clone())
            .collect();
        assert_eq!(Value::from(order), vector["peer_id_order"]);
        // Their placeholder keys and signatures never verify
        let seats = FederationSeats::from_config(&config(&[])).unwrap();
        assert!(matches!(
            verify_for_federation(value, &seats),
            Err(SeatBindingsError::InvalidSignature(0))
        ));
    }
}

#[test]
fn equivalent_but_non_canonical_json_is_rejected() {
    let conformance: Value = serde_json::from_str(CONFORMANCE).unwrap();
    let value = conformance["vectors"][1]["canonical_value"]
        .as_str()
        .unwrap();
    let parsed: Value = serde_json::from_str(value).unwrap();

    let pretty = serde_json::to_string_pretty(&parsed).unwrap();
    assert_eq!(
        parse_canonical(&pretty),
        Err(SeatBindingsError::NonCanonical)
    );

    let mut reordered = parsed.clone();
    reordered["seat_bindings"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert_eq!(
        parse_canonical(&canonical_json(&reordered).unwrap()),
        Err(SeatBindingsError::NonCanonical)
    );

    let mut duplicated = parsed.clone();
    let first = duplicated["seat_bindings"][0].clone();
    duplicated["seat_bindings"]
        .as_array_mut()
        .unwrap()
        .push(first);
    assert_eq!(
        parse_canonical(&canonical_json(&duplicated).unwrap()),
        Err(SeatBindingsError::DuplicatePeerId(0))
    );

    let mut padded_peer = parsed.clone();
    padded_peer["seat_bindings"][0]["attestation"]["peer_id"] = json!("00");
    assert_eq!(
        parse_canonical(&canonical_json(&padded_peer).unwrap()),
        Err(SeatBindingsError::NonCanonicalPeerId)
    );

    let mut extra_field = parsed;
    extra_field["seat_bindings"][0]["attestation"]["note"] = json!("hi");
    assert_eq!(
        parse_canonical(&canonical_json(&extra_field).unwrap()),
        Err(SeatBindingsError::MalformedJson)
    );

    assert_eq!(
        parse_canonical(&directory(vec![])),
        Err(SeatBindingsError::Empty)
    );
    assert_eq!(parse_canonical("{"), Err(SeatBindingsError::MalformedJson));
    assert_eq!(
        parse_canonical(&" ".repeat(MAX_VALUE_BYTES + 1)),
        Err(SeatBindingsError::ValueTooLarge)
    );
}

#[test]
fn a_fully_bound_federation_verifies() {
    let seats = FederationSeats::from_config(&config(&[])).unwrap();
    let verified = verify_for_federation(&directory(valid_bindings(&seats)), &seats).unwrap();
    assert_eq!(
        verified.iter().map(|b| b.peer_id).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(verified[0].fman_pubkey, verified[1].fman_pubkey);
    assert_ne!(verified[1].fman_pubkey, verified[2].fman_pubkey);
    assert_eq!(verified[2].guardian_identity, compressed(3));
    assert_eq!(verified[3].guardian_fee_pubkey, compressed(203));
    assert_eq!(verified[0].issued_at, 1_700_000_000);

    // The config hash ignores meta, so publishing the directory there does
    // not invalidate it
    let with_meta =
        FederationSeats::from_config(&config(&[("federation_name", "Example")])).unwrap();
    assert_eq!(with_meta.config_hash, seats.config_hash);
    assert!(verify_for_federation(&directory(valid_bindings(&seats)), &with_meta).is_ok());
}

#[test]
fn any_bad_binding_invalidates_the_directory() {
    let seats = FederationSeats::from_config(&config(&[])).unwrap();
    let check = |bindings: Vec<Value>| verify_for_federation(&directory(bindings), &seats);

    // Changed after signing
    let mut tampered = valid_bindings(&seats);
    tampered[2]["attestation"]["issued_at"] = json!(1_700_000_001u64);
    assert_eq!(check(tampered), Err(SeatBindingsError::InvalidSignature(2)));

    // Signed by a key other than the one it names
    let mut wrong_signer = valid_bindings(&seats);
    wrong_signer[1] = sign(attestation(&seats, 1, 0), 7);
    assert_eq!(
        check(wrong_signer),
        Err(SeatBindingsError::InvalidSignature(1))
    );

    // The key in uppercase is not the canonical spelling
    let mut upper = attestation(&seats, 3, 2);
    upper["fman_pubkey"] = json!(upper["fman_pubkey"].as_str().unwrap().to_uppercase());
    let mut uppercase_key = valid_bindings(&seats);
    uppercase_key[3] = sign(upper, 2);
    assert_eq!(
        check(uppercase_key),
        Err(SeatBindingsError::InvalidSignature(3))
    );

    let mut other_federation = attestation(&seats, 0, 0);
    other_federation["federation_id"] = json!("00".repeat(32));
    let mut bindings = valid_bindings(&seats);
    bindings[0] = sign(other_federation, 0);
    assert_eq!(
        check(bindings),
        Err(SeatBindingsError::FederationMismatch(0))
    );

    let mut other_config = attestation(&seats, 1, 0);
    other_config["federation_config_hash"] = json!(vec![0u8; 32]);
    let mut bindings = valid_bindings(&seats);
    bindings[1] = sign(other_config, 0);
    assert_eq!(
        check(bindings),
        Err(SeatBindingsError::ConfigHashMismatch(1))
    );

    let mut other_guardian = attestation(&seats, 2, 1);
    other_guardian["guardian_identity"] = json!(compressed(9));
    let mut bindings = valid_bindings(&seats);
    bindings[2] = sign(other_guardian, 1);
    assert_eq!(
        check(bindings),
        Err(SeatBindingsError::GuardianIdentityMismatch(2))
    );

    let mut multisig = attestation(&seats, 3, 2);
    multisig["guardian_fee_account"]["pub_keys"] = json!([compressed(203), compressed(204)]);
    let mut bindings = valid_bindings(&seats);
    bindings[3] = sign(multisig, 2);
    assert_eq!(
        check(bindings),
        Err(SeatBindingsError::InvalidGuardianFeeAccount(3))
    );

    let mut shared_fee = attestation(&seats, 1, 0);
    shared_fee["guardian_fee_account"]["pub_keys"] = json!([compressed(200)]);
    let mut bindings = valid_bindings(&seats);
    bindings[1] = sign(shared_fee, 0);
    assert_eq!(
        check(bindings),
        Err(SeatBindingsError::DuplicateGuardianFeeAccount(1))
    );

    let mut missing_seat = valid_bindings(&seats);
    missing_seat.remove(2);
    assert_eq!(
        check(missing_seat),
        Err(SeatBindingsError::UnboundPeerId(2))
    );

    let mut stranger = attestation(&seats, 0, 3);
    stranger["peer_id"] = json!("4");
    let mut extra_seat = valid_bindings(&seats);
    extra_seat.push(sign(stranger, 3));
    assert_eq!(check(extra_seat), Err(SeatBindingsError::UnknownPeerId(4)));
}

#[test]
fn canonical_json_follows_jcs() {
    let value = json!({ "b": [1, "x\u{1}\"y"], "a": { "z": true, "é": null, "A": 0 } });
    assert_eq!(
        canonical_json(&value).unwrap(),
        r#"{"a":{"A":0,"z":true,"é":null},"b":[1,"x\u0001\"y"]}"#
    );
    assert_eq!(
        canonical_json(&json!(1.5)),
        Err(SeatBindingsError::NonCanonical)
    );
    assert_eq!(
        canonical_json(&json!(-1)),
        Err(SeatBindingsError::NonCanonical)
    );
    assert_eq!(
        canonical_json(&json!(1u64 << 53)),
        Err(SeatBindingsError::NonCanonical)
    );
}
