use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let asset = test_issue_asset_nia(&mut wallet, online, None);
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);

    let consignment_path = wallet.get_send_consignment_path(&asset.asset_id, &txid);
    let consignment = RgbTransfer::load_file(consignment_path).unwrap();
    let message = b"test nonce";
    let signatures = wallet.prove_asset_ownership(&consignment, message).unwrap();
    assert!(!signatures.is_empty());

    let secp = Secp256k1::new();
    let bundle = consignment.bundled_witnesses().last().unwrap();
    let tx = bundle.pub_witness.tx().unwrap();
    for sig in &signatures {
        assert_eq!(sig.outpoint.txid, txid);
        let mut preimage = Vec::new();
        preimage.extend_from_slice(sig.outpoint.txid.as_bytes());
        preimage.extend_from_slice(b":");
        preimage.extend_from_slice(sig.outpoint.vout.to_string().as_bytes());
        preimage.extend_from_slice(b":");
        preimage.extend_from_slice(message);
        let expected_hash: sha256::Hash = Sha256Hash::hash(&preimage);
        assert_eq!(sig.message, expected_hash.to_byte_array());

        // Verify signatures
        let xonly = XOnlyPublicKey::from_slice(&sig.pubkey).unwrap();
        let schnorr_sig =
            bdk_wallet::bitcoin::secp256k1::schnorr::Signature::from_slice(&sig.signature).unwrap();
        let msg =
            bdk_wallet::bitcoin::secp256k1::Message::from_digest(expected_hash.to_byte_array());
        secp.verify_schnorr(&schnorr_sig, &msg, &xonly).unwrap();

        // verify pubkey matches witness TX P2TR output
        let output = tx.output.get(sig.outpoint.vout as usize).unwrap();
        let spk = output.script_pubkey.as_bytes();
        assert_eq!(spk.len(), 34);
        assert_eq!(spk[0], 0x51);
        assert_eq!(spk[1], 0x20);
        assert_eq!(&spk[2..34], sig.pubkey.as_slice());
    }

    // settle the first transfer so change becomes spendable
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);

    // send to self with witness receive, both P2TR outputs should be ours
    let receive_data = test_witness_receive(&mut wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 500,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);
    let consignment_path = wallet.get_send_consignment_path(&asset.asset_id, &txid);
    let consignment = RgbTransfer::load_file(consignment_path).unwrap();
    let signatures = wallet
        .prove_asset_ownership(&consignment, b"self send")
        .unwrap();
    assert_eq!(signatures.len(), 2);
    let vouts: Vec<u32> = signatures.iter().map(|s| s.outpoint.vout).collect();
    let unique_vouts: HashSet<u32> = vouts.iter().copied().collect();
    assert_eq!(vouts.len(), unique_vouts.len());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let amount: u64 = 66;
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    let asset = test_issue_asset_nia(&mut wallet, online, None);
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, online, &recipient_map);
    let consignment_path = wallet.get_send_consignment_path(&asset.asset_id, &txid);
    let consignment = RgbTransfer::load_file(consignment_path).unwrap();

    let (wo_wallet, _wo_online) = get_funded_noutxo_wallet(false, None);
    let result = wo_wallet.prove_asset_ownership(&consignment, b"test");
    assert!(matches!(result, Err(Error::WatchOnly)));

    let runtime = wallet.rgb_runtime().unwrap();
    let contract_id = ContractId::from_str(&asset.asset_id).unwrap();
    let empty_consignment = runtime.transfer(contract_id, [], [], None).unwrap();
    let result = wallet.prove_asset_ownership(&empty_consignment, b"test");
    assert!(matches!(result, Err(Error::NoConsignment)));
}
