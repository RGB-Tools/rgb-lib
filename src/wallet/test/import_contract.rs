use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(Some(&[AMOUNT]));
    let (_destination_dir, contract_path) = party.export_contract_to_tempdir(&asset.asset_id);

    // the asset is unknown to the receiver before the import
    let result = rcv_party.get_asset_metadata_result(&asset.asset_id);
    assert_matches!(result, Err(Error::AssetNotFound { .. }));

    let bak_info_before = rcv_party.db_backup_info();
    rcv_party
        .wallet
        .import_contract(contract_path, vec![])
        .unwrap();
    let bak_info_after = rcv_party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // the asset is now known, with no allocations
    assert_eq!(
        rcv_party.get_asset_metadata(&asset.asset_id),
        party.get_asset_metadata(&asset.asset_id)
    );
    let assets = rcv_party.list_assets(&[]);
    let nia_assets = assets.nia.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(nia_assets[0].asset_id, asset.asset_id);
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id),
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    );
    assert!(rcv_party.list_transfers(Some(&asset.asset_id)).is_empty());

    // the asset can be received through an invoice bound to it
    let receive_data = rcv_party.blind_receive_asset_expiry(Some(asset.asset_id.clone()), None);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_transfer_status_recipient(
        None,
        &receive_data.recipient_id,
        TransferStatus::Settled,
    );
    party.wait_for_transfer_status_sender(None, &txid, TransferStatus::Settled);
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        AMOUNT_SMALL
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn all_schemas() {
    initialize();

    let mut party = get_funded_party!();
    party.create_utxos_default(); // more UTXOs to have free allocation slots for the issuances
    let wallet = get_test_wallet(true, None);

    // the match is exhaustive, so that a new schema has to be sorted
    for schema in AssetSchema::iter() {
        match schema {
            AssetSchema::Nia
            | AssetSchema::Uda
            | AssetSchema::Cfa
            | AssetSchema::Ifa
            | AssetSchema::Bfa => {}
        }
        let asset_id = party.issue_asset_of_schema(schema);
        let (_destination_dir, contract_path) = party.export_contract_to_tempdir(&asset_id);
        wallet.import_contract(contract_path, vec![]).unwrap();
        assert_eq!(
            wallet.get_asset_metadata(asset_id.clone()).unwrap(),
            party.get_asset_metadata(&asset_id),
            "{schema} metadata mismatch"
        );
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn media_success() {
    initialize();

    let mut party = get_funded_party!();
    let rcv_party = get_funded_party!();

    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let uda = party.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str]);
    let cfa = party.issue_asset_cfa(None, Some(FILE_STR.to_string()));
    let (_uda_dir, uda_contract_path) = party.export_contract_to_tempdir(&uda.asset_id);
    let (_cfa_dir, cfa_contract_path) = party.export_contract_to_tempdir(&cfa.asset_id);
    let file_digest = hash_bytes_hex(&fs::read(FILE_STR).unwrap());
    let file_media_path = rcv_party.wallet.get_media_dir().join(&file_digest);

    // the contract cannot be imported without its media, leaving no media behind
    for media_file_paths in [vec![], vec![FILE_STR.to_string()]] {
        let result = rcv_party
            .wallet
            .import_contract(uda_contract_path.clone(), media_file_paths);
        assert_matches!(result, Err(Error::InvalidAttachments { .. }));
        let result = rcv_party.get_asset_metadata_result(&uda.asset_id);
        assert_matches!(result, Err(Error::AssetNotFound { .. }));
        assert!(!file_media_path.exists());
    }

    // with its media (and an extra file, ignored) it's imported, saving the media
    let uda_media_digests = party.asset_media_digests(&uda.asset_id);
    assert_eq!(uda_media_digests.len(), 2);
    let mut media_file_paths: Vec<String> = party
        .wallet
        .list_asset_media(uda.asset_id.clone())
        .unwrap()
        .into_iter()
        .map(|m| m.file_path)
        .collect();
    media_file_paths.push(s!("Cargo.toml"));
    rcv_party
        .wallet
        .import_contract(uda_contract_path, media_file_paths)
        .unwrap();
    assert_eq!(
        rcv_party.asset_media_digests(&uda.asset_id),
        uda_media_digests
    );
    for digest in &uda_media_digests {
        assert!(rcv_party.wallet.get_media_dir().join(digest).exists());
    }
    let rcv_token = rcv_party.get_asset_metadata(&uda.asset_id).token.unwrap();
    let token = party.get_asset_metadata(&uda.asset_id).token.unwrap();
    assert_eq!(rcv_token.media.unwrap().digest, token.media.unwrap().digest);
    assert_eq!(
        rcv_token.attachments.get(&0).unwrap().digest,
        token.attachments.get(&0).unwrap().digest
    );

    // media already in the media directory doesn't need to be provided
    assert!(file_media_path.exists());
    rcv_party
        .wallet
        .import_contract(cfa_contract_path, vec![])
        .unwrap();
    assert_eq!(
        rcv_party.asset_media_digests(&cfa.asset_id),
        HashSet::from([file_digest])
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let nia = party.issue_asset_nia(None);
    let cfa = party.issue_asset_cfa(None, None);
    let (nia_dir, nia_contract_path) = party.export_contract_to_tempdir(&nia.asset_id);
    let (_cfa_dir, cfa_contract_path) = party.export_contract_to_tempdir(&cfa.asset_id);

    // missing file
    let missing_path = format!("missing.{RGB_BIN_EXTENSION}");
    let result = rcv_party
        .wallet
        .import_contract(missing_path.clone(), vec![]);
    assert_matches!(result, Err(Error::InvalidFilePath { file_path }) if file_path == missing_path);

    // not a file
    let dir_path = nia_dir.path().to_string_lossy().to_string();
    let result = rcv_party.wallet.import_contract(dir_path.clone(), vec![]);
    assert_matches!(result, Err(Error::InvalidFilePath { file_path }) if file_path == dir_path);

    // not a contract
    let result = rcv_party
        .wallet
        .import_contract(FILE_STR.to_string(), vec![]);
    assert_matches!(result, Err(Error::InvalidContract { file_path }) if file_path == FILE_STR);

    // a transfer consignment, not a contract one
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT_SMALL),
            recipient_id: receive_data.recipient_id.clone(),
            public_key: Some(receive_data.public_key.clone()),
            expiration_timestamp: Some(receive_data.expiration_timestamp),
            encryption: Encryption::Optional,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    let transfer_path = party
        .wallet
        .get_send_consignment_path(&nia.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let result = rcv_party
        .wallet
        .import_contract(transfer_path.clone(), vec![]);
    assert_matches!(result, Err(Error::InvalidContract { file_path }) if file_path == transfer_path);

    // file too large
    let large_dir = TempDir::new().unwrap();
    let large_path = large_dir.path().join(format!("large.{RGB_BIN_EXTENSION}"));
    fs::File::create(&large_path)
        .unwrap()
        .set_len(MAX_CONTRACT_SIZE + 1)
        .unwrap();
    let large_path = large_path.to_string_lossy().to_string();
    let result = rcv_party.wallet.import_contract(large_path.clone(), vec![]);
    assert_matches!(
        result,
        Err(Error::FileTooLarge { file_path, max_size })
            if file_path == large_path && max_size == MAX_CONTRACT_SIZE
    );

    // asset already known (issued)
    let result = party
        .wallet
        .import_contract(nia_contract_path.clone(), vec![]);
    assert_matches!(result, Err(Error::AssetAlreadyKnown { asset_id }) if asset_id == nia.asset_id);

    // asset already known (imported)
    rcv_party
        .wallet
        .import_contract(nia_contract_path.clone(), vec![])
        .unwrap();
    let result = rcv_party.wallet.import_contract(nia_contract_path, vec![]);
    assert_matches!(result, Err(Error::AssetAlreadyKnown { asset_id }) if asset_id == nia.asset_id);

    // unsupported schema
    let keys = generate_keys(BitcoinNetwork::Regtest, WitnessVersion::Taproot);
    let wallet_nia = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network: BitcoinNetwork::Regtest,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: vec![AssetSchema::Nia],
            allow_reckless_schemas: false,
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let result = wallet_nia.import_contract(cfa_contract_path.clone(), vec![]);
    assert_matches!(
        result,
        Err(Error::UnsupportedSchema {
            asset_schema: AssetSchema::Cfa
        })
    );

    // contract of another network
    let keys = generate_keys(BitcoinNetwork::Testnet, WitnessVersion::Taproot);
    let wallet_testnet = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network: BitcoinNetwork::Testnet,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
            supported_schemas: AssetSchema::VALUES.to_vec(),
            allow_reckless_schemas: false,
        },
        SinglesigKeys::from_keys(&keys, None),
    )
    .unwrap();
    let result = wallet_testnet.import_contract(cfa_contract_path.clone(), vec![]);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));
    let result = wallet_testnet.get_asset_metadata(cfa.asset_id.clone());
    assert_matches!(result, Err(Error::AssetNotFound { .. }));

    // contract with a tampered genesis, missing the global state its schema requires
    let mut contract = Contract::load_file(&cfa_contract_path).unwrap();
    contract.genesis.globals = Default::default();
    let tampered_path = nia_dir.path().join(format!("tampered.{RGB_BIN_EXTENSION}"));
    contract.save_file(&tampered_path).unwrap();
    let result = rcv_party
        .wallet
        .import_contract(tampered_path.to_string_lossy().to_string(), vec![]);
    assert_matches!(result, Err(Error::ConsignmentValidationFailed { .. }));
    let result = rcv_party.get_asset_metadata_result(&contract.contract_id().to_string());
    assert_matches!(result, Err(Error::AssetNotFound { .. }));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ifa_inflated() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // an IFA asset with an inflation
    let asset = party.issue_asset_ifa(Some(&[AMOUNT]), Some(&[AMOUNT_INFLATION]), None);
    party.create_utxos_default();
    party.inflate(&asset.asset_id, &[AMOUNT_INFLATION]);
    mine(false);
    assert!(party.refresh_asset(&asset.asset_id));
    let total_supply = AMOUNT + AMOUNT_INFLATION;
    assert_eq!(
        party
            .get_asset_metadata(&asset.asset_id)
            .known_circulating_supply,
        total_supply
    );

    // the contract carries no inflations
    let (_destination_dir, contract_path) = party.export_contract_to_tempdir(&asset.asset_id);
    rcv_party
        .wallet
        .import_contract(contract_path, vec![])
        .unwrap();
    let metadata = rcv_party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.initial_supply, AMOUNT);
    assert_eq!(metadata.known_circulating_supply, AMOUNT);

    // a transfer spending the inflated allocation too makes the inflation known
    send_and_settle(
        &mut party,
        &mut rcv_party,
        &asset.asset_id,
        Assignment::Fungible(AMOUNT + 1),
    );
    let metadata = rcv_party.get_asset_metadata(&asset.asset_id);
    assert_eq!(metadata.initial_supply, AMOUNT);
    assert_eq!(metadata.known_circulating_supply, total_supply);
}
