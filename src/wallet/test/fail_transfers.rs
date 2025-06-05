use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount = 66;
    let expiration = 1;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // return false if no transfer has changed
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(!test_fail_transfers_all(&mut wallet, &online));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // fail single transfer
    let receive_data = test_blind_receive(&rcv_wallet);
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let bak_info_before = rcv_wallet.database.get_backup_info().unwrap().unwrap();
    assert!(test_fail_transfers_single(
        &mut rcv_wallet,
        &rcv_online,
        receive_data.batch_transfer_idx
    ));
    let bak_info_after = rcv_wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // fail all expired WaitingCounterparty transfers
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            Assignment::Any,
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
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    stop_mining();
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    show_unspent_colorings(&mut rcv_wallet, "receiver run 1 after refresh 1");
    show_unspent_colorings(&mut wallet, "sender run 1 no refresh");
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
    assert!(test_fail_transfers_all(&mut rcv_wallet, &rcv_online));
    show_unspent_colorings(&mut rcv_wallet, "receiver run 1 after fail");
    show_unspent_colorings(&mut wallet, "sender run 1 after fail");
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
    wait_for_refresh(&mut wallet, &online, None, None);
    mine(false, true);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // fail all expired WaitingCounterparty transfers with no asset_id
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = rcv_wallet
        .blind_receive(
            Some(asset.asset_id.clone()),
            Assignment::Any,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_3 = test_blind_receive(&rcv_wallet);
    let receive_data_4 = rcv_wallet
        .blind_receive(
            None,
            Assignment::Any,
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
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);

    show_unspent_colorings(&mut rcv_wallet, "receiver run 2 after refresh 1");
    show_unspent_colorings(&mut wallet, "sender run 2 no refresh");
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
        .fail_transfers(rcv_online, None, true, false)
        .unwrap();
    show_unspent_colorings(&mut rcv_wallet, "receiver run 2 after fail");
    show_unspent_colorings(&mut wallet, "sender run 2 after fail");
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

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn batch_success() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id;

    // transfer is in WaitingCounterparty status and can be failed
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![
            Recipient {
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();
    let txid = send_result.txid;
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
            Some(send_result.batch_transfer_idx),
            false,
            false,
        )
        .unwrap();

    // transfer is still in WaitingCounterparty status after some recipients (but not all) replied with an ACK
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();
    let txid = send_result.txid;
    assert!(!txid.is_empty());
    wait_for_refresh(&mut rcv_wallet_1, &rcv_online_1, None, None);
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
        .fail_transfers(online, Some(send_result.batch_transfer_idx), false, false)
        .unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id.clone();

    // don't fail transfer with asset_id if no_asset_only is true
    let receive_data = wallet
        .blind_receive(
            Some(asset.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let result = wallet.fail_transfers(
        online.clone(),
        Some(receive_data.batch_transfer_idx),
        true,
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // fail pending transfer
    let result = wallet.fail_transfers(
        online.clone(),
        Some(receive_data.batch_transfer_idx),
        false,
        false,
    );
    assert!(result.is_ok());

    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_id = receive_data.recipient_id;
    let batch_transfer_idx = receive_data.batch_transfer_idx;
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            recipient_id: recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(66),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();

    // check starting transfer status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    stop_mining();

    // don't fail unknown idx
    let result = rcv_wallet.fail_transfers(rcv_online.clone(), Some(UNKNOWN_IDX), false, false);
    assert!(matches!(
        result,
        Err(Error::BatchTransferNotFound { idx }) if idx == UNKNOWN_IDX
    ));

    // don't fail incoming transfer: waiting counterparty -> confirmations
    let result =
        rcv_wallet.fail_transfers(rcv_online.clone(), Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting counterparty -> confirmations
    let result = wallet.fail_transfers(
        online.clone(),
        Some(send_result.batch_transfer_idx),
        false,
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // don't fail incoming transfer: waiting confirmations
    let result =
        rcv_wallet.fail_transfers(rcv_online.clone(), Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    // don't fail outgoing transfer: waiting confirmations
    let result = wallet.fail_transfers(
        online.clone(),
        Some(send_result.batch_transfer_idx),
        false,
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // mine and refresh so transfers can settle
    mine(false, true);
    wait_for_refresh(&mut wallet, &online, Some(&asset_id), None);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset_id), None);

    // don't fail incoming transfer: settled
    let result = rcv_wallet.fail_transfers(rcv_online, Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &recipient_id,
        TransferStatus::Settled
    ));
    // don't fail outgoing transfer: settled
    let result = wallet.fail_transfers(online, Some(send_result.batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &recipient_id,
        TransferStatus::Settled
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn batch_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(
        &mut wallet,
        &online,
        Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]),
    );
    let asset_id = asset.asset_id;

    // batch send as donation (doesn't wait for recipient confirmations)
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset_id,
        vec![
            Recipient {
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                assignment: Assignment::Fungible(amount),
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
            false,
        )
        .unwrap();

    // transfer is in WaitingConfirmations status and cannot be failed
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    let result = wallet.fail_transfers(
        online,
        Some(receive_data_2.batch_transfer_idx),
        false,
        false,
    );
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount = 66;
    let expiration = 1;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // fail single transfer skipping sync
    let receive_data = test_blind_receive(&rcv_wallet);
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        rcv_wallet
            .fail_transfers(
                rcv_online.clone(),
                Some(receive_data.batch_transfer_idx),
                false,
                true
            )
            .unwrap()
    );

    // fail all expired WaitingCounterparty transfers
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            Assignment::Any,
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
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    stop_mining();
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    show_unspent_colorings(&mut rcv_wallet, "receiver run 1 after refresh 1");
    show_unspent_colorings(&mut wallet, "sender run 1 no refresh");
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
    assert!(
        rcv_wallet
            .fail_transfers(rcv_online.clone(), None, false, true)
            .unwrap()
    );
    show_unspent_colorings(&mut rcv_wallet, "receiver run 1 after fail");
    show_unspent_colorings(&mut wallet, "sender run 1 after fail");
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
    wait_for_refresh(&mut wallet, &online, None, None);
    mine(false, true);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // fail all expired WaitingCounterparty transfers with no asset_id
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = rcv_wallet
        .blind_receive(
            Some(asset.asset_id.clone()),
            Assignment::Any,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_3 = test_blind_receive(&rcv_wallet);
    let receive_data_4 = rcv_wallet
        .blind_receive(
            None,
            Assignment::Any,
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
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            assignment: Assignment::Fungible(amount),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);

    show_unspent_colorings(&mut rcv_wallet, "receiver run 2 after refresh 1");
    show_unspent_colorings(&mut wallet, "sender run 2 no refresh");
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
        .fail_transfers(rcv_online, None, true, true)
        .unwrap();
    show_unspent_colorings(&mut rcv_wallet, "receiver run 2 after fail");
    show_unspent_colorings(&mut wallet, "sender run 2 after fail");
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
