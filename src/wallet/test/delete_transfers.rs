use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // return false if no transfer has changed
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(!test_delete_transfers(&wallet, None, None, false));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // delete single transfer
    let receive_data = test_blind_receive(&mut wallet);
    test_fail_transfers_blind(&mut wallet, &online, &receive_data.recipient_id);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(test_delete_transfers(
        &wallet,
        Some(&receive_data.recipient_id),
        None,
        false
    ));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // delete all Failed transfers
    let receive_data_1 = test_blind_receive(&mut wallet);
    let receive_data_2 = test_blind_receive(&mut wallet);
    let receive_data_3 = test_blind_receive(&mut wallet);
    test_fail_transfers_blind(&mut wallet, &online, &receive_data_1.recipient_id);
    test_fail_transfers_blind(&mut wallet, &online, &receive_data_2.recipient_id);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
    show_unspent_colorings(&wallet, "run 1 before delete");
    test_delete_transfers(&wallet, None, None, false);
    show_unspent_colorings(&wallet, "run 1 after delete");
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 1);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // fail and delete remaining pending transfers
    assert!(test_fail_transfers_blind(
        &mut wallet,
        &online,
        &receive_data_3.recipient_id
    ));
    assert!(test_delete_transfers(
        &wallet,
        Some(&receive_data_3.recipient_id),
        None,
        false
    ));
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 0);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data_1 = test_blind_receive(&mut wallet);
    let receive_data_2 = wallet
        .blind_receive(
            Some(asset.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(test_fail_transfers_blind(
        &mut wallet,
        &online,
        &receive_data_1.recipient_id
    ));
    assert!(test_fail_transfers_blind(
        &mut wallet,
        &online,
        &receive_data_2.recipient_id
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
    show_unspent_colorings(&wallet, "run 2 before delete");
    assert!(test_delete_transfers(&wallet, None, None, true));
    show_unspent_colorings(&wallet, "run 2 after delete");
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 2);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
}

#[test]
#[parallel]
fn batch_success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id;

    // failed transfer can be deleted, using both blinded_utxo + txid
    let receive_data_1 = test_blind_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&mut rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    test_fail_transfers_txid(&mut wallet, &online, &txid);
    test_delete_transfers(
        &wallet,
        Some(&receive_data_1.recipient_id),
        Some(&txid),
        false,
    );

    // ...and can be deleted using txid only
    let receive_data_1 = test_blind_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&mut rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    test_fail_transfers_txid(&mut wallet, &online, &txid);
    test_delete_transfers(&wallet, None, Some(&txid), false);
}

#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let receive_data = test_blind_receive(&mut wallet);

    // don't delete transfer not in Failed status
    assert!(!check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    let result =
        test_delete_transfers_result(&wallet, Some(&receive_data.recipient_id), None, false);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // don't delete unknown blinded UTXO
    let result = test_delete_transfers_result(&wallet, Some("txob1inexistent"), None, false);
    assert!(matches!(
        result,
        Err(Error::TransferNotFound { recipient_id: _ })
    ));

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    show_unspent_colorings(&wallet, "after issuance");

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data = wallet
        .blind_receive(
            Some(asset.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    test_fail_transfers_all(&mut wallet, &online);
    let result =
        test_delete_transfers_result(&wallet, Some(&receive_data.recipient_id), None, true);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));
}

#[test]
#[parallel]
fn batch_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id;

    // only blinded UTXO given but multiple transfers in batch
    let receive_data_1 = test_blind_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&mut rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    test_fail_transfers_txid(&mut wallet, &online, &txid);
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::Failed
    ));
    let result =
        test_delete_transfers_result(&wallet, Some(&receive_data_1.recipient_id), None, false);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // blinded UTXO + txid given but blinded UTXO transfer not part of batch transfer
    let receive_data_1 = test_blind_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&mut rcv_wallet_2);
    let recipient_map_1 = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid_1 = test_send(&mut wallet, &online, &recipient_map_1);
    test_fail_transfers_txid(&mut wallet, &online, &txid_1);
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_1,
        TransferStatus::Failed
    ));
    let receive_data_3 = test_blind_receive(&mut rcv_wallet_2);
    let recipient_map_2 = HashMap::from([(
        asset_id,
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_3.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet, &online, &recipient_map_2);
    test_fail_transfers_txid(&mut wallet, &online, &txid_2);
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_2,
        TransferStatus::Failed
    ));
    let result = test_delete_transfers_result(
        &wallet,
        Some(&receive_data_3.recipient_id),
        Some(&txid_1),
        false,
    );
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));
}
