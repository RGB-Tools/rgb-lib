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

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset_1 = test_issue_asset_nia(&mut wallet_1, &online_1, Some(&[AMOUNT, AMOUNT]));
    let asset_2 = test_issue_asset_nia(&mut wallet_2, &online_2, Some(&[AMOUNT * 2, AMOUNT * 2]));

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let receive_data_2a = test_blind_receive(&wallet_2);
    let recipient_map_1a = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_2a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // return false if no transfer has changed
    let bak_info_before = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(!test_refresh_all(&mut wallet_2, &online_2));
    let bak_info_after = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let txid_1a = test_send(&mut wallet_1, &online_1, &recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = test_blind_receive(&wallet_1);
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_1a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2a = test_send(&mut wallet_2, &online_2, &recipient_map_2a);
    assert!(!txid_2a.is_empty());
    assert!(test_refresh_all(&mut wallet_1, &online_1));
    let bak_info_before = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(test_refresh_all(&mut wallet_2, &online_2));
    let bak_info_after = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(
        test_refresh_result(
            &mut wallet_1,
            &online_1,
            Some(&asset_1.asset_id),
            &[filter_counter_out.clone()]
        )
        .unwrap()
        .transfers_changed()
    );
    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = test_blind_receive(&wallet_2);
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id,
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_2b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1b = test_send(&mut wallet_1, &online_1, &recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = test_blind_receive(&wallet_1);
    show_unspent_colorings(&mut wallet_1, "wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_1b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2b = test_send(&mut wallet_2, &online_2, &recipient_map_2b);
    assert!(!txid_2b.is_empty());
    show_unspent_colorings(&mut wallet_2, "wallet 2 after send 2b");
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh incoming WaitingCounterparty only (wallet 1)
    assert!(
        test_refresh_result(&mut wallet_1, &online_1, None, &[filter_counter_in])
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_1,
        "wallet 1 after refresh incoming WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // refresh outgoing WaitingCounterparty only (wallet 2)
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        test_refresh_result(&mut wallet_2, &online_2, None, &[filter_counter_out])
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_2,
        "wallet 2 after refresh outgoing WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    mine(false, true);

    // refresh incoming WaitingConfirmations only (wallet 2)
    assert!(
        test_refresh_result(&mut wallet_2, &online_2, None, &[filter_confirm_in])
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_2,
        "wallet 2 after refresh incoming WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh outgoing WaitingConfirmations only (wallet 1)
    assert!(
        test_refresh_result(&mut wallet_1, &online_1, None, &[filter_confirm_out])
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_1,
        "wallet 1 after refresh outgoing WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // asset not found
    let result = test_refresh_result(&mut wallet, &online, Some("rgb1inexistent"), &[]);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn nia_with_media() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    let fp = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let fpath = std::path::Path::new(&fp);
    let file_bytes = std::fs::read(fp.clone()).unwrap();
    let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
    let digest = file_hash.to_byte_array();
    let mime = FileFormat::from_file(fpath)
        .unwrap()
        .media_type()
        .to_string();
    let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
    let media_type = MediaType::with(media_ty);
    let attachment = Attachment {
        ty: media_type,
        digest: digest.into(),
    };
    println!("setting MOCK_CONTRACT_DATA");
    MOCK_CONTRACT_DATA.lock().unwrap().push(attachment.clone());
    let asset = test_issue_asset_nia(&mut wallet_1, &online_1, None);
    let media_idx = wallet_1
        .copy_media_and_save(
            fp,
            &Media::from_attachment(&attachment, wallet_1.get_media_dir()),
        )
        .unwrap();
    let db_asset = wallet_1
        .database
        .get_asset(asset.asset_id.clone())
        .unwrap()
        .unwrap();
    let mut updated_asset: DbAssetActMod = db_asset.into();
    updated_asset.media_idx = ActiveValue::Set(Some(media_idx));
    block_on(
        crate::database::entities::asset::Entity::update(updated_asset)
            .exec(wallet_1.database.get_connection()),
    )
    .unwrap();

    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let assets_list = test_list_assets(&wallet_2, &[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);

    let receive_data = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    let assets_list = test_list_assets(&wallet_3, &[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn nia_with_details() {
    initialize();

    let amount: u64 = 66;
    let details_str = "mocked details";

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // manually set the asset's details
    let details = Some(details_str);
    println!("setting MOCK_CONTRACT_DETAILS");
    *MOCK_CONTRACT_DETAILS.lock().unwrap() = details;

    // issue
    let asset = test_issue_asset_nia(&mut wallet_1, &online_1, None);

    // check asset details have been set
    assert_eq!(asset.details, Some(details_str.to_string()));

    // send 1->2
    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    // settle transfer
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);

    // send 2->3
    let receive_data = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    // settle transfer
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check asset details on the final recipient
    let asset_list = test_list_assets(&wallet_3, &[]);
    assert_eq!(
        asset_list.nia.unwrap()[0].details,
        Some(details_str.to_string())
    );
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn uda_with_preview_and_reserves() {
    initialize();

    let amount: u64 = 1;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    let data = vec![1u8, 3u8, 9u8];
    let preview_ty = "text/plain";
    let preview = RgbEmbeddedMedia {
        ty: MediaType::with(preview_ty),
        data: Confined::try_from(data.clone()).unwrap(),
    };
    let proof = vec![2u8, 4u8, 6u8, 10u8];
    let reserves = RgbProofOfReserves {
        utxo: RgbOutpoint::from_str(FAKE_TXID).unwrap(),
        proof: Confined::try_from(proof.clone()).unwrap(),
    };
    let token_data = TokenData {
        index: TokenIndex::from_inner(UDA_FIXED_INDEX),
        ticker: Some(Ticker::from(TICKER)),
        name: Some(Name::from(NAME)),
        details: Some(Details::from(DETAILS)),
        preview: Some(preview),
        media: None,
        attachments: Confined::try_from(BTreeMap::new()).unwrap(),
        reserves: Some(reserves),
    };
    println!("setting MOCK_TOKEN_DATA");
    MOCK_TOKEN_DATA.lock().unwrap().push(token_data.clone());
    let asset = test_issue_asset_uda(&mut wallet_1, &online_1, Some(DETAILS), None, vec![]);

    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let assets_list = test_list_assets(&wallet_2, &[]);
    assert!(
        assets_list.uda.unwrap()[0]
            .token
            .as_ref()
            .unwrap()
            .media
            .is_none()
    );
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);

    let receive_data = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    let assets_list = test_list_assets(&wallet_3, &[]);
    let uda = assets_list.uda.unwrap();
    let token = uda[0].token.as_ref().unwrap();
    assert_eq!(token.index, UDA_FIXED_INDEX);
    assert_eq!(token.ticker, Some(TICKER.to_string()));
    assert_eq!(token.name, Some(NAME.to_string()));
    assert_eq!(token.details, Some(DETAILS.to_string()));
    assert!(token.embedded_media);
    assert!(token.media.is_none());
    assert_eq!(token.attachments, HashMap::new());
    assert!(token.reserves);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let uda_metadata = test_get_asset_metadata(&wallet_3, &asset.asset_id);
    assert_eq!(uda_metadata.asset_schema, AssetSchema::Uda);
    assert_eq!(uda_metadata.issued_supply, 1);
    assert_eq!(uda_metadata.name, NAME.to_string());
    assert_eq!(uda_metadata.precision, PRECISION);
    assert_eq!(uda_metadata.ticker, Some(TICKER.to_string()));
    assert_eq!(uda_metadata.details, Some(DETAILS.to_string()));
    let token = uda_metadata.token.unwrap();
    let embedded_media = token.embedded_media.unwrap();
    assert_eq!(embedded_media.mime, preview_ty);
    assert_eq!(embedded_media.data, data);
    let reserves = token.reserves.unwrap();
    assert_eq!(reserves.utxo.to_string(), FAKE_TXID);
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

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset_1 = test_issue_asset_nia(&mut wallet_1, &online_1, Some(&[AMOUNT, AMOUNT]));
    let asset_2 = test_issue_asset_nia(&mut wallet_2, &online_2, Some(&[AMOUNT * 2, AMOUNT * 2]));

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let receive_data_2a = test_blind_receive(&wallet_2);
    let recipient_map_1a = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_2a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // refresh skipping sync > no transfer has changed so return false before and after syncing
    assert!(
        !wallet_2
            .refresh(online_2.clone(), None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    wallet_2.sync(online_2.clone()).unwrap();
    assert!(
        !wallet_2
            .refresh(online_2.clone(), None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    let txid_1a = test_send(&mut wallet_1, &online_1, &recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = test_blind_receive(&wallet_1);
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_1a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2a = test_send(&mut wallet_2, &online_2, &recipient_map_2a);
    assert!(!txid_2a.is_empty());
    // refresh skipping sync > transfers have changed so return true
    assert!(
        wallet_1
            .refresh(online_1.clone(), None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    assert!(
        wallet_2
            .refresh(online_2.clone(), None, vec![], true)
            .unwrap()
            .transfers_changed()
    );
    assert!(
        wallet_1
            .refresh(
                online_1.clone(),
                Some(asset_1.asset_id.to_string()),
                vec![filter_counter_out.clone()],
                true,
            )
            .unwrap()
            .transfers_changed()
    );

    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = test_blind_receive(&wallet_2);
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id,
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_2b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1b = test_send(&mut wallet_1, &online_1, &recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = test_blind_receive(&wallet_1);
    show_unspent_colorings(&mut wallet_1, "wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_1b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2b = test_send(&mut wallet_2, &online_2, &recipient_map_2b);
    assert!(!txid_2b.is_empty());
    show_unspent_colorings(&mut wallet_2, "wallet 2 after send 2b");
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh incoming WaitingCounterparty only (wallet 1), skipping sync
    assert!(
        wallet_1
            .refresh(online_1.clone(), None, vec![filter_counter_in], true)
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_1,
        "wallet 1 after refresh incoming WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // refresh outgoing WaitingCounterparty only (wallet 2), skipping sync
    assert!(
        wallet_2
            .refresh(online_2.clone(), None, vec![filter_counter_out], true)
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_2,
        "wallet 2 after refresh outgoing WaitingCounterparty",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    mine(false, true);

    // refresh incoming WaitingConfirmations only (wallet 2), skipping sync
    assert!(
        wallet_2
            .refresh(online_2.clone(), None, vec![filter_confirm_in], true)
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_2,
        "wallet 2 after refresh incoming WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2a,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_2,
        &txid_2b,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2a.recipient_id,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_2,
        &receive_data_2b.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // refresh outgoing WaitingConfirmations only (wallet 1), skipping sync
    assert!(
        wallet_1
            .refresh(online_1.clone(), None, vec![filter_confirm_out], true)
            .unwrap()
            .transfers_changed()
    );
    show_unspent_colorings(
        &mut wallet_1,
        "wallet 1 after refresh outgoing WaitingConfirmations",
    );
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1a,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_sender(
        &wallet_1,
        &txid_1b,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1a.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet_1,
        &receive_data_1b.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
}
