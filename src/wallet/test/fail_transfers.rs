use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let amount = 66;
    let expiration = 1;

    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, rcv_online) = get_funded_wallet!();

    // return false if no transfer has changed
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(!test_fail_transfers_all(&wallet, &online));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // issue
    let asset = test_issue_asset_nia(&wallet, &online, None);

    // fail single transfer
    let receive_data = test_blind_receive(&rcv_wallet);
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let bak_info_before = rcv_wallet.database.get_backup_info().unwrap().unwrap();
    assert!(test_fail_transfers_blind(
        &rcv_wallet,
        &rcv_online,
        &receive_data.recipient_id
    ));
    let bak_info_after = rcv_wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // fail all expired WaitingCounterparty transfers
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = test_blind_receive(&rcv_wallet);
    let receive_data_3 = test_blind_receive(&rcv_wallet);
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_3.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    stop_mining();
    test_refresh_all(&rcv_wallet, &rcv_online);
    show_unspent_colorings(&rcv_wallet, "receiver run 1 after refresh 1");
    show_unspent_colorings(&wallet, "sender run 1 no refresh");
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(test_fail_transfers_all(&rcv_wallet, &rcv_online));
    show_unspent_colorings(&rcv_wallet, "receiver run 1 after fail");
    show_unspent_colorings(&wallet, "sender run 1 after fail");
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // progress transfer to Settled
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    mine(true);
    test_refresh_all(&rcv_wallet, &rcv_online);
    wallet.refresh(online.clone(), None, vec![]).unwrap();

    // fail all expired WaitingCounterparty transfers with no asset_id
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = rcv_wallet
        .blind_receive(
            Some(asset.asset_id.clone()),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_3 = test_blind_receive(&rcv_wallet);
    let receive_data_4 = rcv_wallet
        .blind_receive(
            None,
            None,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));
    // progress transfer 3 to WaitingConfirmations
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_3.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    test_refresh_all(&rcv_wallet, &rcv_online);

    show_unspent_colorings(&rcv_wallet, "receiver run 2 after refresh 1");
    show_unspent_colorings(&wallet, "sender run 2 no refresh");
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_4.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    rcv_wallet
        .fail_transfers(rcv_online, None, None, true)
        .unwrap();
    show_unspent_colorings(&rcv_wallet, "receiver run 2 after fail");
    show_unspent_colorings(&wallet, "sender run 2 after fail");
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data_4.recipient_id,
        TransferStatus::Failed
    ));
}

#[test]
#[parallel]
fn batch_success() {
    initialize();

    let amount = 66;

    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&wallet, &online, None);
    let asset_id = asset.asset_id;

    // transfer is in WaitingCounterparty status and can be failed, using both blinded_utxo + txid
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));
    wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data_1.recipient_id),
            Some(txid),
            false,
        )
        .unwrap();

    // ...and can be failed using txid only
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    test_fail_transfers_txid(&wallet, &online, &txid);

    // transfer is still in WaitingCounterparty status after some recipients (but not all) replied with an ACK
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    let txid = test_send(&wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    rcv_wallet_1.refresh(rcv_online_1, None, vec![]).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));
    wallet
        .fail_transfers(online, Some(receive_data_2.recipient_id), Some(txid), false)
        .unwrap();
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&wallet, &online, None);
    let asset_id = asset.asset_id.clone();

    // don't fail transfer with asset_id if no_asset_only is true
    let receive_data = wallet
        .blind_receive(
            Some(asset.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let result = wallet.fail_transfers(
        online.clone(),
        Some(receive_data.recipient_id.clone()),
        None,
        true,
    );
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    // fail pending blind
    let result =
        wallet.fail_transfers(online.clone(), Some(receive_data.recipient_id), None, false);
    assert!(result.is_ok());

    // blind
    let receive_data = test_blind_receive(&rcv_wallet);
    let blinded_utxo = receive_data.recipient_id;
    // send
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&blinded_utxo).unwrap(),
            ),
            amount: 66,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send(&wallet, &online, &recipient_map);

    // check starting transfer status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));

    stop_mining();

    // don't fail unknown blinded UTXO
    let result =
        rcv_wallet.fail_transfers(rcv_online.clone(), Some(s!("txob1inexistent")), None, false);
    assert!(matches!(
        result,
        Err(Error::TransferNotFound { recipient_id: _ })
    ));

    // don't fail incoming transfer: waiting counterparty -> confirmations
    let result =
        rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting counterparty -> confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // don't fail incoming transfer: waiting confirmations
    let result =
        rcv_wallet.fail_transfers(rcv_online.clone(), Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting confirmations
    let result = wallet.fail_transfers(online.clone(), Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // mine and refresh so transfers can settle
    mine(true);
    test_refresh_asset(&wallet, &online, &asset_id);
    test_refresh_asset(&rcv_wallet, &rcv_online, &asset_id);

    // don't fail incoming transfer: settled
    let result = rcv_wallet.fail_transfers(rcv_online, Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
    // don't fail outgoing transfer: settled
    let result = wallet.fail_transfers(online, Some(blinded_utxo.clone()), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &blinded_utxo,
        TransferStatus::Settled
    ));
}

#[test]
#[parallel]
fn batch_fail() {
    initialize();

    let amount = 66;

    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&wallet, &online, Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]));
    let asset_id = asset.asset_id;

    // only blinded utxo given but multiple transfers in batch
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    let txid = test_send(&wallet, &online, &recipient_map);
    let result = wallet.fail_transfers(
        online.clone(),
        Some(receive_data_1.recipient_id),
        None,
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    // successfully fail transfer so asset can be spent again
    let result = wallet.fail_transfers(online.clone(), None, Some(txid), false);
    assert!(result.is_ok());

    // blinded utxo + txid given but blinded utxo transfer not part of batch transfer
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    let txid_1 = test_send(&wallet, &online, &recipient_map_1);
    let receive_data_3 = test_blind_receive(&rcv_wallet_2);
    let recipient_map_2 = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_3.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&wallet, &online, &recipient_map_2);
    let result = wallet.fail_transfers(
        online.clone(),
        Some(receive_data_3.recipient_id),
        Some(txid_1.clone()),
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
    // successfully fail transfers so asset can be spent again
    test_fail_transfers_txid(&wallet, &online, &txid_1);
    test_fail_transfers_txid(&wallet, &online, &txid_2);

    // batch send as donation (doesn't wait for recipient confirmations)
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
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
    wallet
        .send(
            online.clone(),
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // transfer is in WaitingConfirmations status and cannot be failed
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    let result = wallet.fail_transfers(online, Some(receive_data_2.recipient_id), None, false);
    assert!(matches!(result, Err(Error::CannotFailTransfer)));
}
