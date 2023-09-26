use super::*;
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

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset_1 = wallet_1
        .issue_asset_nia(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
        )
        .unwrap();
    let asset_2 = wallet_2
        .issue_asset_nia(
            online_2.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION,
            vec![AMOUNT * 2, AMOUNT * 2],
        )
        .unwrap();

    // per each wallet prepare:
    // - 1 WaitingCounterparty + 1 WaitingConfirmations ountgoing
    // - 1 WaitingCounterparty + 1 WaitingConfirmations incoming

    stop_mining();

    // wallet 1 > wallet 2 WaitingConfirmations and vice versa
    let receive_data_2a = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    assert!(!wallet_2.refresh(online_2.clone(), None, vec![]).unwrap());
    let txid_1a = test_send_default(&mut wallet_1, &online_1, recipient_map_1a);
    assert!(!txid_1a.is_empty());
    let receive_data_1a = wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid_2a = test_send_default(&mut wallet_2, &online_2, recipient_map_2a);
    assert!(!txid_2a.is_empty());
    assert!(wallet_1.refresh(online_1.clone(), None, vec![]).unwrap());
    assert!(wallet_2.refresh(online_2.clone(), None, vec![]).unwrap());
    assert!(wallet_1
        .refresh(
            online_1.clone(),
            Some(asset_1.asset_id.clone()),
            vec![filter_counter_out.clone()],
        )
        .unwrap());
    // wallet 1 > 2, WaitingCounterparty and vice versa
    let receive_data_2b = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid_1b = test_send_default(&mut wallet_1, &online_1, recipient_map_1b);
    assert!(!txid_1b.is_empty());
    // wallet 2 > 1, WaitingCounterparty
    let receive_data_1b = wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid_2b = test_send_default(&mut wallet_2, &online_2, recipient_map_2b);
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

    let (mut wallet, online) = get_funded_wallet!();

    // asset not found
    let result = wallet.refresh(online, Some(s!("rgb1inexistent")), vec![]);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}

#[test]
#[serial]
fn nia_with_media() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    let fp = "tests/qrcode.png".to_string();
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
    let asset = wallet_1
        .issue_asset_nia(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let attachment_id = hex::encode(media.digest);
    let media_dir = wallet_1
        .wallet_dir
        .join(ASSETS_DIR)
        .join(asset.asset_id.clone())
        .join(attachment_id);
    fs::create_dir_all(&media_dir).unwrap();
    let media_path = media_dir.join(MEDIA_FNAME);
    fs::copy(fp, media_path).unwrap();
    fs::write(media_dir.join(MIME_FNAME), mime).unwrap();

    let receive_data = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet_1, &online_1, recipient_map);
    assert!(!txid.is_empty());

    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    let assets_list = wallet_2.list_assets(vec![]).unwrap();
    assert_eq!(assets_list.nia.unwrap()[0].data_paths.len(), 1);
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();

    let receive_data = wallet_3
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet_2, &online_2, recipient_map);
    assert!(!txid.is_empty());

    wallet_3.refresh(online_3.clone(), None, vec![]).unwrap();
    let assets_list = wallet_3.list_assets(vec![]).unwrap();
    assert_eq!(assets_list.nia.unwrap()[0].data_paths.len(), 1);
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
