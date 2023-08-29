use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // return false if no transfer has changed
    assert!(!wallet.delete_transfers(None, None, false).unwrap());

    // delete single transfer
    let receive_data = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data.recipient_id.clone()),
            None,
            false,
        )
        .unwrap();
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    assert!(wallet
        .delete_transfers(Some(receive_data.recipient_id), None, false)
        .unwrap());

    // delete all Failed transfers
    let receive_data_1 = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_3 = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data_1.recipient_id.clone()),
            None,
            false,
        )
        .unwrap();
    wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data_2.recipient_id.clone()),
            None,
            false,
        )
        .unwrap();
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
    wallet.delete_transfers(None, None, false).unwrap();
    show_unspent_colorings(&wallet, "run 1 after delete");
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 1);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // fail and delete remaining pending tranfers
    assert!(wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data_3.recipient_id.clone()),
            None,
            false,
        )
        .unwrap());
    assert!(wallet
        .delete_transfers(Some(receive_data_3.recipient_id), None, false)
        .unwrap());
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 0);

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data_1 = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = wallet
        .blind_receive(
            Some(asset.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data_1.recipient_id.clone()),
            None,
            false,
        )
        .unwrap());
    assert!(wallet
        .fail_transfers(
            online,
            Some(receive_data_2.recipient_id.clone()),
            None,
            false
        )
        .unwrap());
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
    assert!(wallet.delete_transfers(None, None, true).unwrap());
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
fn batch_success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;

    // failed transfer can be deleted, using both blinded_utxo + txid
    let receive_data_1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online.clone(), None, Some(txid.clone()), false)
        .unwrap();
    wallet
        .delete_transfers(Some(receive_data_1.recipient_id), Some(txid), false)
        .unwrap();

    // ...and can be deleted using txid only
    let receive_data_1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());
    wallet
        .fail_transfers(online, None, Some(txid.clone()), false)
        .unwrap();
    wallet.delete_transfers(None, Some(txid), false).unwrap();
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let receive_data = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // don't delete transfer not in Failed status
    assert!(!check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(receive_data.recipient_id), None, false);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // don't delete unknown blinded UTXO
    let result = wallet.delete_transfers(Some(s!("txob1inexistent")), None, false);
    assert!(matches!(
        result,
        Err(Error::TransferNotFound { recipient_id: _ })
    ));

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
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
    wallet.fail_transfers(online, None, None, false).unwrap();
    let result = wallet.delete_transfers(Some(receive_data.recipient_id), None, true);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));
}

#[test]
fn batch_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;

    // only blinded UTXO given but multiple transfers in batch
    let receive_data_1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    wallet
        .fail_transfers(online.clone(), None, Some(txid.clone()), false)
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(receive_data_1.recipient_id), None, false);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));

    // blinded UTXO + txid given but blinded UTXO transfer not part of batch transfer
    let receive_data_1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid_1 = test_send_default(&mut wallet, &online, recipient_map_1);
    wallet
        .fail_transfers(online.clone(), None, Some(txid_1.clone()), false)
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_1,
        TransferStatus::Failed
    ));
    let receive_data_3 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
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
    let txid_2 = test_send_default(&mut wallet, &online, recipient_map_2);
    wallet
        .fail_transfers(online, None, Some(txid_2.clone()), false)
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_2,
        TransferStatus::Failed
    ));
    let result = wallet.delete_transfers(Some(receive_data_3.recipient_id), Some(txid_1), false);
    assert!(matches!(result, Err(Error::CannotDeleteTransfer)));
}
