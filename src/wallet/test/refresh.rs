use super::*;
use rgbstd::interface::rgb21::EmbeddedMedia as RgbEmbeddedMedia;
use rgbstd::stl::ProofOfReserves as RgbProofOfReserves;
use serial_test::{parallel, serial};

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

    let (wallet_1, online_1) = get_funded_wallet!();
    let (wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset_1 = test_issue_asset_nia(&wallet_1, &online_1, Some(&[AMOUNT, AMOUNT]));
    let asset_2 = test_issue_asset_nia(&wallet_2, &online_2, Some(&[AMOUNT * 2, AMOUNT * 2]));

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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2a.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // return false if no transfer has changed
    let bak_info_before = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(!wallet_2.refresh(online_2.clone(), None, vec![]).unwrap());
    let bak_info_after = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let txid_1a = test_send(&wallet_1, &online_1, &recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = test_blind_receive(&wallet_1);
    let recipient_map_2a = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1a.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2a = test_send(&wallet_2, &online_2, &recipient_map_2a);
    assert!(!txid_2a.is_empty());
    assert!(wallet_1.refresh(online_1.clone(), None, vec![]).unwrap());
    let bak_info_before = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(wallet_2.refresh(online_2.clone(), None, vec![]).unwrap());
    let bak_info_after = wallet_2.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(wallet_1
        .refresh(
            online_1.clone(),
            Some(asset_1.asset_id.clone()),
            vec![filter_counter_out.clone()],
        )
        .unwrap());
    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = test_blind_receive(&wallet_2);
    let recipient_map_1b = HashMap::from([(
        asset_1.asset_id,
        vec![Recipient {
            amount: amount_1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2b.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1b = test_send(&wallet_1, &online_1, &recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = test_blind_receive(&wallet_1);
    show_unspent_colorings(&wallet_1, "wallet 1 after blind 1b");
    let recipient_map_2b = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1b.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2b = test_send(&wallet_2, &online_2, &recipient_map_2b);
    assert!(!txid_2b.is_empty());
    show_unspent_colorings(&wallet_2, "wallet 2 after send 2b");
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
    assert!(wallet_1
        .refresh(online_1.clone(), None, vec![filter_counter_in])
        .unwrap());
    show_unspent_colorings(
        &wallet_1,
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
    assert!(wallet_2
        .refresh(online_2.clone(), None, vec![filter_counter_out])
        .unwrap());
    show_unspent_colorings(
        &wallet_2,
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

    mine(true);

    // refresh incoming WaitingConfirmations only (wallet 2)
    assert!(wallet_2
        .refresh(online_2, None, vec![filter_confirm_in])
        .unwrap());
    show_unspent_colorings(
        &wallet_2,
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
    assert!(wallet_1
        .refresh(online_1, None, vec![filter_confirm_out])
        .unwrap());
    show_unspent_colorings(
        &wallet_1,
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

#[test]
#[parallel]
fn fail() {
    initialize();

    let (wallet, online) = get_funded_wallet!();

    // asset not found
    let result = wallet.refresh(online, Some(s!("rgb1inexistent")), vec![]);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}

#[test]
#[serial]
fn nia_with_media() {
    initialize();

    let amount: u64 = 66;

    let (wallet_1, online_1) = get_funded_wallet!();
    let (wallet_2, online_2) = get_funded_wallet!();
    let (wallet_3, online_3) = get_funded_wallet!();

    let fp = ["tests", "qrcode.png"].join(&MAIN_SEPARATOR.to_string());
    let fpath = std::path::Path::new(&fp);
    let file_bytes = std::fs::read(fp.clone()).unwrap();
    let file_hash: sha256::Hash = Sha256Hash::hash(&file_bytes[..]);
    let digest = file_hash.to_byte_array();
    let mime = tree_magic::from_filepath(fpath);
    let media_ty: &'static str = Box::leak(mime.clone().into_boxed_str());
    let media_type = MediaType::with(media_ty);
    let media = Attachment {
        ty: media_type,
        digest,
    };
    MOCK_CONTRACT_DATA.lock().unwrap().push(media.clone());
    let asset = test_issue_asset_nia(&wallet_1, &online_1, None);
    let digest = hex::encode(media.digest);
    let media_dir = wallet_1.wallet_dir.join(MEDIA_DIR);
    fs::create_dir_all(&media_dir).unwrap();
    fs::copy(fp, media_dir.join(digest)).unwrap();

    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    let assets_list = test_list_assets(&wallet_2, &[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();

    let receive_data = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    wallet_3.refresh(online_3.clone(), None, vec![]).unwrap();
    let assets_list = test_list_assets(&wallet_3, &[]);
    assert!(assets_list.nia.unwrap()[0].media.is_some());
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_3.refresh(online_3, None, vec![]).unwrap();
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[test]
#[serial]
fn uda_with_preview_and_reserves() {
    initialize();

    let amount: u64 = 1;

    let (wallet_1, online_1) = get_funded_wallet!();
    let (wallet_2, online_2) = get_funded_wallet!();
    let (wallet_3, online_3) = get_funded_wallet!();

    let index_int = 7;
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
        index: TokenIndex::from_inner(index_int),
        ticker: Some(Ticker::try_from(TICKER).unwrap()),
        name: Some(Name::try_from(NAME).unwrap()),
        details: Some(Details::try_from(DETAILS).unwrap()),
        preview: Some(preview),
        media: None,
        attachments: Confined::try_from(BTreeMap::new()).unwrap(),
        reserves: Some(reserves),
    };
    MOCK_TOKEN_DATA.lock().unwrap().push(token_data.clone());
    let asset = test_issue_asset_uda(&wallet_1, &online_1, None, vec![]);

    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    let assets_list = test_list_assets(&wallet_2, &[]);
    assert!(assets_list.uda.unwrap()[0]
        .token
        .as_ref()
        .unwrap()
        .media
        .is_none());
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();

    let receive_data = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    wallet_3.refresh(online_3.clone(), None, vec![]).unwrap();
    let assets_list = test_list_assets(&wallet_3, &[]);
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
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_3.refresh(online_3, None, vec![]).unwrap();
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let uda_metadata = test_get_asset_metadata(&wallet_3, &asset.asset_id);
    assert_eq!(uda_metadata.asset_iface, AssetIface::RGB21);
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
