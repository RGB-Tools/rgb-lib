use super::*;

fn attach(consignment_path: &str) -> Result<SpvProofsAttachment, Error> {
    attach_spv_proofs(
        consignment_path,
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
    )
}

/// Validate the consignment with the lookup of the mined witnesses refused, so that it can only
/// succeed through the proofs the consignment carries and the block headers.
fn validate_spv_only(consignment_path: &str) -> Result<ConsignmentValidation, Error> {
    MOCK_SPV_ONLY.replace(true);
    let result = validate_consignment(
        consignment_path,
        &IndexerConfig::Electrum {
            url: ELECTRUM_URL.to_string(),
        },
        &HashMap::new(),
        Some(1),
        &ValidationOptions {
            bitcoin_network: BitcoinNetwork::Regtest,
            offchain_txid: None,
        },
    );
    MOCK_SPV_ONLY.replace(false);
    result
}

/// Issue an asset and send it to a blinded UTXO of the recipient, returning the asset ID, the
/// TXID and the path of the consignment.
fn send(party: &mut SinglesigParty, rcv_party: &mut SinglesigParty) -> (String, String, String) {
    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
        }],
    )]);
    let txid = party.send(recipient_map, FEE_RATE, None).txid;
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    (asset.asset_id, txid, consignment_path)
}

#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // the consignment carries no proof, its only witness is not mined yet
    let (asset_id, txid, consignment_path) = send(&mut party, &mut rcv_party);
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    let consignment_id = consignment.consignment_id().to_string();
    assert!(
        consignment
            .bundled_witnesses()
            .all(|bw| bw.spv_proof.is_none())
    );

    // no proof can be retrieved for an unmined witness: the file is left as it is
    let file_content = fs::read(&consignment_path).unwrap();
    let attachment = attach(&consignment_path).unwrap();
    assert_eq!(
        attachment,
        SpvProofsAttachment {
            consignment_id: consignment_id.clone(),
            attached: vec![],
            missing: vec![txid.clone()],
        }
    );
    assert_eq!(fs::read(&consignment_path).unwrap(), file_content);

    // settle the transfer
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_id));
    mine(false);

    // without its proof the mined witness can only be validated by looking it up
    let result = validate_spv_only(&consignment_path);
    assert_matches!(result, Err(Error::Network { .. }));

    // the proof of the mined witness gets attached, which changes the consignment ID
    let attachment = attach(&consignment_path).unwrap();
    assert_eq!(attachment.attached, vec![txid.clone()]);
    assert!(attachment.missing.is_empty());
    assert_ne!(attachment.consignment_id, consignment_id);
    let consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    assert_eq!(
        attachment.consignment_id,
        consignment.consignment_id().to_string()
    );
    assert!(
        consignment
            .bundled_witnesses()
            .all(|bw| bw.spv_proof.is_some())
    );
    assert!(!Path::new(&format!("{consignment_path}.tmp")).exists());

    // the consignment is now validated via the proof and the block header alone
    let validation = validate_spv_only(&consignment_path).unwrap();
    assert_eq!(validation.consignment_id, attachment.consignment_id);
    assert_matches!(
        validation.witnesses.as_slice(),
        [ValidatedWitness {
            txid: t,
            status: WitnessValidationStatus::Mined { .. },
            from_consignment: false,
        }] if *t == txid
    );
    assert!(validation.warnings.is_empty());

    // nothing is left to attach: the file is left as it is
    let file_content = fs::read(&consignment_path).unwrap();
    let attachment_again = attach(&consignment_path).unwrap();
    assert_eq!(
        attachment_again,
        SpvProofsAttachment {
            consignment_id: attachment.consignment_id,
            attached: vec![],
            missing: vec![],
        }
    );
    assert_eq!(fs::read(&consignment_path).unwrap(), file_content);

    // a contract consignment carries the genesis only, so it has no witness to prove
    let contract_file = tempfile::NamedTempFile::with_prefix("attach_spv_proofs::").unwrap();
    let contract = consignment.into_contract();
    contract.save_file(contract_file.path()).unwrap();
    let attachment = attach(&contract_file.path().to_string_lossy()).unwrap();
    assert_eq!(
        attachment,
        SpvProofsAttachment {
            consignment_id: contract.consignment_id().to_string(),
            attached: vec![],
            missing: vec![],
        }
    );
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // inexistent file
    let result = attach("inexistent");
    assert_matches!(result, Err(Error::InvalidFilePath { file_path: f }) if f == "inexistent");

    // not a consignment
    let invalid_file = tempfile::NamedTempFile::with_prefix("attach_spv_proofs::").unwrap();
    let invalid_path = invalid_file.path().to_string_lossy().to_string();
    fs::write(&invalid_file, b"not a consignment").unwrap();
    let result = attach(&invalid_path);
    assert_matches!(result, Err(Error::InvalidConsignment));

    // a schema definition
    AssetSchema::Nia
        .schema_definition()
        .save_file(invalid_file.path())
        .unwrap();
    let result = attach(&invalid_path);
    assert_matches!(result, Err(Error::InvalidConsignment));

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let (_, _, consignment_path) = send(&mut party, &mut rcv_party);
    let file_content = fs::read(&consignment_path).unwrap();

    // unreachable indexer
    let result = attach_spv_proofs(
        &consignment_path,
        &IndexerConfig::Electrum {
            url: s!("127.0.0.1:1"),
        },
    );
    assert_matches!(result, Err(Error::InvalidIndexer { .. }));
    assert_eq!(fs::read(&consignment_path).unwrap(), file_content);
}
