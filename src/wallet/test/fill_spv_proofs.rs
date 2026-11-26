use super::*;

#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    // a first transfer, settled, so its witness is mined and the proof its sender retrieves for it
    // travels in the consignment of the next one
    let receive_data_1 = rcv_party.blind_receive();
    let txid_1 = send_to_blind_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data_1,
        Assignment::Fungible(AMOUNT_SMALL),
    );

    // the receiver spends what it received, with no mining in the meanwhile: the consignment has
    // two witnesses, the mined one of the first transfer, carrying a proof, and its own, which
    // has none as it isn't mined yet
    let receive_data_2 = party.blind_receive();
    let recipient_map = blind_recipient_map(
        &asset.asset_id,
        &receive_data_2,
        Assignment::Fungible(AMOUNT_SMALL / 2),
    );
    let miner_stop_guard = stop_mining();
    let txid_2 = rcv_party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = rcv_party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid_2)
        .to_string_lossy()
        .to_string();
    let proof_1 = witness_spv_proof(&consignment_path, &txid_1).unwrap();
    assert!(witness_spv_proof(&consignment_path, &txid_2).is_none());

    // a partial fill: the proven witness isn't looked up and the other has no proof to retrieve
    // yet, so the consignment is left as it is
    let fill = fill_proofs(&consignment_path);
    assert_eq!(fill.without_proof.len(), 1);
    assert_eq!(fill.without_proof[0].txid, txid_2);
    assert!(!fill.rewritten);
    assert_eq!(witness_spv_proof(&consignment_path, &txid_1), Some(proof_1));
    assert!(witness_spv_proof(&consignment_path, &txid_2).is_none());
    drop(miner_stop_guard);

    // settle the second transfer, so its witness gets mined as well
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party.wait_for_transfer_status_recipient(
        None,
        &receive_data_2.recipient_id,
        TransferStatus::Settled,
    );
    rcv_party.wait_for_refresh(Some(&asset.asset_id));

    // the missing proof is now retrieved, which leaves the consignment fully proven
    let fill = fill_proofs(&consignment_path);
    assert!(fill.without_proof.is_empty());
    assert!(fill.rewritten);
    let proof_2 = witness_spv_proof(&consignment_path, &txid_2).unwrap();

    // filling in again looks up no witness, as they're all proven, and leaves the file as it is
    let fill = fill_proofs(&consignment_path);
    assert!(fill.without_proof.is_empty());
    assert!(!fill.rewritten);
    assert_eq!(
        witness_spv_proof(&consignment_path, &txid_2),
        Some(proof_2.clone())
    );

    // the consignment validates via the proofs alone, as witness TX lookups are refused
    MOCK_SPV_ONLY.replace(true);
    let validation = validate_test_consignment(&consignment_path, None, Some(MIN_CONFIRMATIONS));
    MOCK_SPV_ONLY.replace(false);
    let validation = validation.unwrap();
    assert!(validation.warnings.is_empty());
    let witness_2 = validation
        .witnesses
        .iter()
        .find(|w| w.txid == txid_2)
        .unwrap();
    assert_matches!(
        witness_2.status,
        WitnessValidationStatus::Mined { height, .. } if height == proof_2.block_height.get()
    );

    // a proof already there is kept, even if it no longer verifies, as only the missing ones are
    // retrieved. The proof is made stale by altering a sibling, as the position alone wouldn't
    // always do: when the witness is the last TX of a level with an odd number of nodes, its
    // sibling is itself, so its position can change by one without changing the merkle root
    let mut stale_proof = proof_2.clone();
    let sibling = stale_proof.merkle.get_mut(0).unwrap();
    *sibling = Sha256Hash::from_byte_array(sibling.to_byte_array().map(|b| !b));
    let witness_id = RgbTxid::from_str(&txid_2).unwrap();
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    assert!(consignment.modify_bundle(witness_id, |witness_bundle| {
        witness_bundle.spv_proof = Some(stale_proof.clone())
    }));
    consignment.save_file(&consignment_path).unwrap();
    let fill = fill_proofs(&consignment_path);
    assert!(fill.without_proof.is_empty());
    assert!(!fill.rewritten);
    assert_eq!(
        witness_spv_proof(&consignment_path, &txid_2),
        Some(stale_proof)
    );

    // validation then resolves that witness without the stale proof, reporting it
    let validation =
        validate_test_consignment(&consignment_path, None, Some(MIN_CONFIRMATIONS)).unwrap();
    assert_matches!(
        &validation.warnings[..],
        [ValidationWarning::InvalidSpvProof { txid, .. }] if *txid == txid_2
    );

    // dropping the stale proof gets the valid one back
    assert!(consignment.modify_bundle(witness_id, |witness_bundle| {
        witness_bundle.spv_proof = None
    }));
    consignment.save_file(&consignment_path).unwrap();
    let fill = fill_proofs(&consignment_path);
    assert!(fill.without_proof.is_empty());
    assert!(fill.rewritten);
    assert_eq!(witness_spv_proof(&consignment_path, &txid_2), Some(proof_2));
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let txid = send_to_blind_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        &receive_data,
        Assignment::Fungible(AMOUNT_SMALL),
    );
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();

    // inexistent file
    let result = fill_spv_proofs("inexistent", &electrum_indexer_config());
    assert!(matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent"));

    // a contract consignment, which holds the genesis only, so it has no witness to prove
    let contract_file = tempfile::NamedTempFile::with_prefix("fill_spv_proofs::").unwrap();
    RgbTransfer::load_file(&consignment_path)
        .unwrap()
        .into_contract()
        .save_file(contract_file.path())
        .unwrap();
    let result = fill_spv_proofs(
        &contract_file.path().to_string_lossy(),
        &electrum_indexer_config(),
    );
    assert_matches!(result, Err(Error::InvalidConsignment));

    // indexer for another network than the one of the consignment's asset
    let result = fill_spv_proofs(
        &consignment_path,
        &IndexerConfig::Electrum {
            url: ELECTRUM_SIGNET_CUSTOM_URL.to_string(),
        },
    );
    assert_matches!(result, Err(Error::InvalidIndexer { .. }));
}
