use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    let filter_counter_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: true,
    };
    let filter_counter_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: false,
    };
    let filter_confirm_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: true,
    };
    let filter_confirm_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: false,
    };

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset_1 = party_1.issue_asset_nia(Some(&[AMOUNT, AMOUNT]));
    let asset_2 = party_2.issue_asset_nia(Some(&[AMOUNT * 2, AMOUNT * 2]));

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    let _guard = stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let receive_data_2a = party_2.blind_receive();
    let recipient_map_1a = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_2a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // return false if no transfer has changed
    let bak_info_before = party_2.db_backup_info();
    assert!(!party_2.refresh_all());
    let bak_info_after = party_2.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let txid_1a = party_1.send_retry(&recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = party_1.blind_receive();
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_1a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2a = party_2.send_retry(&recipient_map_2a);
    assert!(!txid_2a.is_empty());
    assert!(party_1.refresh_all());
    let bak_info_before = party_2.db_backup_info();
    assert!(party_2.refresh_all());
    let bak_info_after = party_2.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(
        party_1
            .refresh_result(
                Some(&asset_1.asset_id),
                std::slice::from_ref(&filter_counter_out),
            )
            .unwrap()
            .transfers_changed()
    );
    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = party_2.blind_receive();
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_2b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1b = party_1.send_retry(&recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = party_1.blind_receive();
    party_1.show_unspent_colorings("wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_1b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2b = party_2.send_retry(&recipient_map_2b);
    assert!(!txid_2b.is_empty());
    party_2.show_unspent_colorings("wallet 2 after send 2b");
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh incoming WaitingCounterparty only (wallet 1)
    assert!(
        party_1
            .refresh_result(None, &[filter_counter_in])
            .unwrap()
            .transfers_changed()
    );
    party_1.show_unspent_colorings("wallet 1 after refresh incoming WaitingCounterparty");
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // refresh outgoing WaitingCounterparty only (wallet 2)
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingCounterparty)
    );
    assert!(
        party_2
            .refresh_result(None, &[filter_counter_out])
            .unwrap()
            .transfers_changed()
    );
    party_2.show_unspent_colorings("wallet 2 after refresh outgoing WaitingCounterparty");
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingConfirmations)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    drop(_guard);
    mine(false);
    mine_tx(false, &txid_2a);
    mine_tx(false, &txid_2b);

    // refresh incoming WaitingConfirmations only (wallet 2)
    assert!(
        party_2
            .refresh_result(None, &[filter_confirm_in])
            .unwrap()
            .transfers_changed()
    );
    party_2.show_unspent_colorings("wallet 2 after refresh incoming WaitingConfirmations");
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingConfirmations)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::Settled
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh outgoing WaitingConfirmations only (wallet 1)
    assert!(
        party_1
            .refresh_result(None, &[filter_confirm_out])
            .unwrap()
            .transfers_changed()
    );
    party_1.show_unspent_colorings("wallet 1 after refresh outgoing WaitingConfirmations");
    assert!(party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::Settled));
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // === offline tests

    let mut offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let result = offline_party
        .wallet
        .refresh(Online { id: 0 }, None, vec![], false);
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let mut party = get_funded_party!();

    // asset not found
    let result = party.refresh_result(Some("rgb1inexistent"), &[]);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn nia_with_media() {
    initialize();

    let amount: u64 = 66;

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    let fp = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let fpath = std::path::Path::new(&fp);
    let file_bytes = std::fs::read(fp.clone()).unwrap();
    let digest = hash_bytes(&file_bytes[..]);
    let mime = FileFormat::from_file(fpath)
        .unwrap()
        .media_type()
        .to_string();
    let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
    let media_type = MediaType::with(media_ty);
    let attachment = Attachment {
        ty: media_type,
        digest: Bytes32::try_from(digest.as_slice()).unwrap(),
    };
    println!("setting MOCK_CONTRACT_DATA");
    MOCK_CONTRACT_DATA.with_borrow_mut(|v| v.push(attachment.clone()));
    let asset = party_1.issue_asset_nia(None);
    let media = Media::from_attachment(&attachment, party_1.wallet.get_media_dir());
    party_1.wallet.copy_media_file(fp, &media).unwrap();
    let media_idx = party_1.db_get_or_insert_media(&media.get_digest(), &media.mime);
    let db_asset = party_1.db_asset(&asset.asset_id);
    let mut updated_asset: DbAssetActMod = db_asset.into();
    updated_asset.media_idx = ActiveValue::Set(Some(media_idx));
    party_1.db_update_asset(&mut updated_asset);

    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_2.wait_for_refresh(None);
    let assets_list = party_2.list_assets(&[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_3.wait_for_refresh(None);
    let assets_list = party_3.list_assets(&[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    let rcv_transfer = party_3.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_3.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_2.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party_2.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn nia_with_details() {
    initialize();

    let amount: u64 = 66;
    let details_str = "mocked details";

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    // manually set the asset's details
    println!("setting MOCK_CONTRACT_DETAILS");
    MOCK_CONTRACT_DETAILS.replace(Some(details_str.to_string()));

    // issue
    let asset = party_1.issue_asset_nia(None);

    // check asset details have been set
    assert_eq!(asset.details, Some(details_str.to_string()));

    // send 1->2
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // settle transfer
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // send 2->3
    let receive_data = party_3.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // settle transfer
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    let rcv_transfer = party_3.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_3.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_2.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party_2.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check asset details on the final recipient
    let asset_list = party_3.list_assets(&[]);
    assert_eq!(
        asset_list.nia.unwrap()[0].details,
        Some(details_str.to_string())
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn uda_with_media() {
    initialize();

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    let fp = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let fpath = std::path::Path::new(&fp);
    let file_bytes = std::fs::read(fp.clone()).unwrap();
    let digest = hash_bytes(&file_bytes[..]);
    let mime = FileFormat::from_file(fpath)
        .unwrap()
        .media_type()
        .to_string();
    let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
    let media_type = MediaType::with(media_ty);
    let attachment = Attachment {
        ty: media_type,
        digest: Bytes32::try_from(digest.as_slice()).unwrap(),
    };
    println!("setting MOCK_CONTRACT_DATA");
    MOCK_CONTRACT_DATA.with_borrow_mut(|v| v.push(attachment.clone()));
    let asset = party_1.issue_asset_uda(None, Some(FILE_STR), vec![]);
    let media = Media::from_attachment(&attachment, party_1.wallet.get_media_dir());
    party_1.wallet.copy_media_file(fp, &media).unwrap();
    let media_idx = party_1.db_get_or_insert_media(&media.get_digest(), &media.mime);
    let mut updated_asset: DbAssetActMod = party_1.db_asset(&asset.asset_id).into();
    updated_asset.media_idx = ActiveValue::Set(Some(media_idx));
    party_1.db_update_asset(&mut updated_asset);

    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_2.wait_for_refresh(None);
    let assets_list = party_2.list_assets(&[]);
    assert!(assets_list.uda.unwrap()[0].media.is_some());
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_3.wait_for_refresh(None);
    let assets_list = party_3.list_assets(&[]);
    assert!(assets_list.uda.unwrap()[0].media.is_some());
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    let rcv_transfer = party_3.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_3.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_2.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party_2.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn uda_with_preview_and_reserves() {
    initialize();

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    let index_int = 7;
    let data = vec![1u8, 3u8, 9u8];
    let preview_ty = "text/plain";
    let preview = RgbEmbeddedMedia {
        ty: MediaType::with(preview_ty),
        data: Confined::try_from(data.clone()).unwrap(),
    };
    let proof = vec![2u8, 4u8, 6u8, 10u8];
    let reserves = RgbProofOfReserves {
        utxo: OutPoint::from_str(FAKE_OUTPOINT).unwrap(),
        proof: Confined::try_from(proof.clone()).unwrap(),
    };
    let token_data = TokenData {
        index: TokenIndex::from_inner(index_int),
        ticker: Some(Ticker::from(TICKER)),
        name: Some(Name::from(NAME)),
        details: Some(Details::from(DETAILS)),
        preview: Some(preview),
        media: None,
        attachments: Confined::try_from(BTreeMap::new()).unwrap(),
        reserves: Some(reserves),
    };
    println!("setting MOCK_TOKEN_DATA");
    MOCK_TOKEN_DATA.with_borrow_mut(|v| v.push(token_data.clone()));
    let asset = party_1.issue_asset_uda(Some(DETAILS), None, vec![]);

    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_2.wait_for_refresh(None);
    let assets_list = party_2.list_assets(&[]);
    assert!(
        assets_list.uda.unwrap()[0]
            .token
            .as_ref()
            .unwrap()
            .media
            .is_none()
    );
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party_3.wait_for_refresh(None);
    let assets_list = party_3.list_assets(&[]);
    let uda = assets_list.uda.unwrap();
    let token = uda[0].token.as_ref().unwrap();
    assert_eq!(token.index, index_int);
    assert_eq!(token.ticker, Some(TICKER.to_string()));
    assert_eq!(token.name, Some(NAME.to_string()));
    assert_eq!(token.details, Some(DETAILS.to_string()));
    assert!(token.embedded_media);
    assert!(token.media.is_none());
    assert_eq!(token.attachments, HashMap::new());
    assert!(token.reserves);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    let rcv_transfer = party_3.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_3.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_2.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party_2.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let uda_metadata = party_3.get_asset_metadata(&asset.asset_id);
    assert_eq!(uda_metadata.asset_schema, AssetSchema::Uda);
    assert_eq!(uda_metadata.initial_supply, 1);
    assert_eq!(uda_metadata.name, NAME.to_string());
    assert_eq!(uda_metadata.precision, PRECISION);
    assert_eq!(uda_metadata.ticker, Some(TICKER.to_string()));
    assert_eq!(uda_metadata.details, Some(DETAILS.to_string()));
    let token = uda_metadata.token.unwrap();
    let embedded_media = token.embedded_media.unwrap();
    assert_eq!(embedded_media.mime, preview_ty);
    assert_eq!(embedded_media.data, data);
    let reserves = token.reserves.unwrap();
    assert_eq!(reserves.utxo.to_string(), FAKE_OUTPOINT);
    assert_eq!(reserves.proof, proof);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    let filter_counter_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: true,
    };
    let filter_counter_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: false,
    };
    let filter_confirm_in = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: true,
    };
    let filter_confirm_out = RefreshFilter {
        status: RefreshTransferStatus::WaitingConfirmations,
        incoming: false,
    };

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset_1 = party_1.issue_asset_nia(Some(&[AMOUNT, AMOUNT]));
    let asset_2 = party_2.issue_asset_nia(Some(&[AMOUNT * 2, AMOUNT * 2]));

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    let _guard = stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let receive_data_2a = party_2.blind_receive();
    let recipient_map_1a = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_2a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // refresh skipping sync > no transfer has changed so return false before and after syncing
    assert!(
        !party_2
            .wallet
            .refresh(party_2.online, None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    party_2
        .wallet
        .sync(
            party_2.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    assert!(
        !party_2
            .wallet
            .refresh(party_2.online, None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    let txid_1a = party_1.send_retry(&recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = party_1.blind_receive();
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_1a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2a = party_2.send_retry(&recipient_map_2a);
    assert!(!txid_2a.is_empty());
    // refresh skipping sync > transfers have changed so return true
    assert!(
        party_1
            .wallet
            .refresh(party_1.online, None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    assert!(
        party_2
            .wallet
            .refresh(party_2.online, None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    assert!(
        party_1
            .wallet
            .refresh(
                party_1.online,
                Some(asset_1.asset_id.to_string()),
                vec![filter_counter_out.clone()],
                true,
            )
            .unwrap()
            .transfers_changed()
    );

    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = party_2.blind_receive();
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_2b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1b = party_1.send_retry(&recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = party_1.blind_receive();
    party_1.show_unspent_colorings("wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_1b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2b = party_2.send_retry(&recipient_map_2b);
    assert!(!txid_2b.is_empty());
    party_2.show_unspent_colorings("wallet 2 after send 2b");
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh incoming WaitingCounterparty only (wallet 1), skipping sync
    assert!(
        party_1
            .wallet
            .refresh(party_1.online, None, vec![filter_counter_in], true)
            .unwrap()
            .transfers_changed()
    );
    party_1.show_unspent_colorings("wallet 1 after refresh incoming WaitingCounterparty");
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // refresh outgoing WaitingCounterparty only (wallet 2), skipping sync
    assert!(
        party_2
            .wallet
            .refresh(party_2.online, None, vec![filter_counter_out], true)
            .unwrap()
            .transfers_changed()
    );
    party_2.show_unspent_colorings("wallet 2 after refresh outgoing WaitingCounterparty");
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingConfirmations)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    drop(_guard);
    mine(false);

    // refresh incoming WaitingConfirmations only (wallet 2), skipping sync
    assert!(
        party_2
            .wallet
            .refresh(party_2.online, None, vec![filter_confirm_in], true)
            .unwrap()
            .transfers_changed()
    );
    party_2.show_unspent_colorings("wallet 2 after refresh incoming WaitingConfirmations");
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2a, TransferStatus::WaitingConfirmations)
    );
    assert!(
        party_2.check_test_transfer_status_sender(&txid_2b, TransferStatus::WaitingConfirmations)
    );
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2a.recipient_id,
        TransferStatus::Settled
    ));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh outgoing WaitingConfirmations only (wallet 1), skipping sync
    assert!(
        party_1
            .wallet
            .refresh(party_1.online, None, vec![filter_confirm_out], true)
            .unwrap()
            .transfers_changed()
    );
    party_1.show_unspent_colorings("wallet 1 after refresh outgoing WaitingConfirmations");
    assert!(party_1.check_test_transfer_status_sender(&txid_1a, TransferStatus::Settled));
    assert!(
        party_1.check_test_transfer_status_sender(&txid_1b, TransferStatus::WaitingCounterparty)
    );
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party_1.check_test_transfer_status_recipient(
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn filter_with_waiting_safe_height() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_nia(None);

    // 1st transfer: wallet 1 > wallet 2 (settle it to give txid_1 a single confirmation)
    let receive_data_1 = party_2.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map_1);
    assert!(!txid_1.is_empty());
    let _guard = stop_mining_when_alone();
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    force_mine_no_resume_when_alone(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Settled
    ));

    // 2nd transfer: wallet 1 > wallet 2 with min_confirmations = 2
    // txid_1 has only one confirmation, so the transfer parks in WaitingSafeHeight
    let receive_data_2 = party_2
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64),
            TRANSPORT_ENDPOINTS.clone(),
            2,
        )
        .unwrap();
    let recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_1.send_retry(&recipient_map_2);
    assert!(!txid_2.is_empty());

    // transfer parks in WaitingSafeHeight because it contains unsafe history
    party_2.wait_for_refresh_raw(None, Some(&[receive_data_2.batch_transfer_idx]));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingSafeHeight
    ));

    // refreshing with a non-empty filter must not panic on the WaitingSafeHeight transfer;
    // the transfer doesn't match the filter, so it's skipped and nothing changes
    let filter = RefreshFilter {
        status: RefreshTransferStatus::WaitingCounterparty,
        incoming: true,
    };
    let refresh_res = party_2.refresh_result(None, &[filter]);
    assert!(!refresh_res.unwrap().transfers_changed());

    // refreshing with a non-empty filter that includes WaitingSafeHeight must not panic;
    // the transfer matches the filter, so it's refreshed but status remains the same because we didn't mine
    let filter = RefreshFilter {
        status: RefreshTransferStatus::WaitingSafeHeight,
        incoming: true,
    };
    let refresh_res = party_2.refresh_result(None, std::slice::from_ref(&filter));
    assert!(!refresh_res.unwrap().transfers_changed());

    // mine a block so the transfer reaches safe height
    force_mine_no_resume_when_alone(false);
    let refresh_res = party_2.refresh_result(None, &[filter]);
    assert!(refresh_res.unwrap().transfers_changed());
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}
