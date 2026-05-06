use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount = 66;
    let expiration_secs: u64 = 1;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // return false if no transfer has changed
    let bak_info_before = party.db_backup_info();
    assert!(!party.fail_transfers_all());
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // issue
    let asset = party.issue_asset_nia(None);

    // fail single transfer
    let receive_data = rcv_party.blind_receive();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let bak_info_before = rcv_party.db_backup_info();
    assert!(rcv_party.fail_transfers_single(receive_data.batch_transfer_idx));
    let bak_info_after = rcv_party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // fail all expired WaitingCounterparty transfers
    let receive_data_1 = rcv_party.blind_receive_asset_expiry(
        None,
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    let receive_data_2 = rcv_party.blind_receive();
    let receive_data_3 = rcv_party.blind_receive();
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration_secs * 1000 + 2000,
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
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let _guard = stop_mining();
    rcv_party.wait_for_refresh(None);
    rcv_party.show_unspent_colorings("receiver run 1 after refresh 1");
    party.show_unspent_colorings("sender run 1 no refresh");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.fail_transfers_all());
    rcv_party.show_unspent_colorings("receiver run 1 after fail");
    party.show_unspent_colorings("sender run 1 after fail");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // progress transfer to Settled
    party.wait_for_refresh(None);
    drop(_guard);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // fail all expired WaitingCounterparty transfers with no asset_id
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive_asset_expiry(
        Some(asset.asset_id.clone()),
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    let receive_data_3 = rcv_party.blind_receive();
    let receive_data_4 = rcv_party.blind_receive_asset_expiry(
        None,
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration_secs * 1000 + 2000,
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
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    rcv_party.wait_for_refresh(None);

    rcv_party.show_unspent_colorings("receiver run 2 after refresh 1");
    party.show_unspent_colorings("sender run 2 no refresh");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_4.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    rcv_party.fail_transfers(None, true, false).unwrap();
    rcv_party.show_unspent_colorings("receiver run 2 after fail");
    party.show_unspent_colorings("sender run 2 after fail");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
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

    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);
    let asset_id = asset.asset_id;

    // transfer is in WaitingCounterparty status and can be failed
    let receive_data_1 = rcv_party_1.blind_receive();
    let receive_data_2 = rcv_party_2.blind_receive();
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
    let send_result = party.send_result(&recipient_map).unwrap();
    let txid = send_result.txid;
    assert!(!txid.is_empty());
    assert!(rcv_party_1.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingCounterparty));
    party
        .fail_transfers(Some(send_result.batch_transfer_idx), false, false)
        .unwrap();

    // transfer is still in WaitingCounterparty status after some recipients (but not all) replied with an ACK
    let receive_data_1 = rcv_party_1.blind_receive();
    let receive_data_2 = rcv_party_2.blind_receive();
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
    let send_result = party.send_result(&recipient_map).unwrap();
    let txid = send_result.txid;
    assert!(!txid.is_empty());
    rcv_party_1.wait_for_refresh(None);
    assert!(rcv_party_1.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingCounterparty));
    party
        .fail_transfers(Some(send_result.batch_transfer_idx), false, false)
        .unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);
    let asset_id = asset.asset_id.clone();

    // don't fail transfer with asset_id if no_asset_only is true
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let result = party.fail_transfers(Some(receive_data.batch_transfer_idx), true, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // fail pending transfer
    let result = party.fail_transfers(Some(receive_data.batch_transfer_idx), false, false);
    assert!(result.is_ok());

    let receive_data = rcv_party.blind_receive();
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
    let send_result = party.send_result(&recipient_map).unwrap();

    // check starting transfer status
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingCounterparty
        )
    );
    assert!(
        party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingCounterparty
        )
    );

    let _guard = stop_mining();

    // don't fail unknown idx
    let result = rcv_party.fail_transfers(Some(UNKNOWN_IDX), false, false);
    assert!(matches!(
        result,
        Err(Error::BatchTransferNotFound { idx }) if idx == UNKNOWN_IDX
    ));

    // don't fail incoming transfer: waiting counterparty -> confirmations
    let result = rcv_party.fail_transfers(Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingConfirmations
        )
    );
    // don't fail outgoing transfer: waiting counterparty -> confirmations
    let result = party.fail_transfers(Some(send_result.batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingConfirmations
        )
    );

    // don't fail incoming transfer: waiting confirmations
    let result = rcv_party.fail_transfers(Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingConfirmations
        )
    );
    // don't fail outgoing transfer: waiting confirmations
    let result = party.fail_transfers(Some(send_result.batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        party.check_test_transfer_status_recipient(
            &recipient_id,
            TransferStatus::WaitingConfirmations
        )
    );

    // mine and refresh so transfers can settle
    drop(_guard);
    mine(false);
    party.wait_for_refresh(Some(&asset_id));
    rcv_party.wait_for_refresh(Some(&asset_id));

    // don't fail incoming transfer: settled
    let result = rcv_party.fail_transfers(Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(rcv_party.check_test_transfer_status_recipient(&recipient_id, TransferStatus::Settled));
    // don't fail outgoing transfer: settled
    let result = party.fail_transfers(Some(send_result.batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(party.check_test_transfer_status_recipient(&recipient_id, TransferStatus::Settled));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn batch_fail() {
    initialize();

    let amount = 66;

    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]));
    let asset_id = asset.asset_id;

    // batch send as donation (doesn't wait for recipient confirmations)
    let receive_data_1 = rcv_party_1.blind_receive();
    let receive_data_2 = rcv_party_2.blind_receive();
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
    party
        .wallet
        .send(
            party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();

    // transfer is in WaitingConfirmations status and cannot be failed
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    let result = party.fail_transfers(Some(receive_data_2.batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount = 66;
    let expiration_secs: u64 = 1;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // fail single transfer skipping sync
    let receive_data = rcv_party.blind_receive();
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        rcv_party
            .fail_transfers(Some(receive_data.batch_transfer_idx), false, true)
            .unwrap()
    );

    // fail all expired WaitingCounterparty transfers
    let receive_data_1 = rcv_party.blind_receive_asset_expiry(
        None,
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    let receive_data_2 = rcv_party.blind_receive();
    let receive_data_3 = rcv_party.blind_receive();
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration_secs * 1000 + 2000,
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
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let _guard = stop_mining();
    rcv_party.wait_for_refresh(None);
    rcv_party.show_unspent_colorings("receiver run 1 after refresh 1");
    party.show_unspent_colorings("sender run 1 no refresh");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.fail_transfers(None, false, true).unwrap());
    rcv_party.show_unspent_colorings("receiver run 1 after fail");
    party.show_unspent_colorings("sender run 1 after fail");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // progress transfer to Settled
    party.wait_for_refresh(None);
    drop(_guard);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // fail all expired WaitingCounterparty transfers with no asset_id
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive_asset_expiry(
        Some(asset.asset_id.clone()),
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    let receive_data_3 = rcv_party.blind_receive();
    let receive_data_4 = rcv_party.blind_receive_asset_expiry(
        None,
        Some((now().unix_timestamp() + expiration_secs as i64) as u64),
    );
    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration_secs * 1000 + 2000,
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
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    rcv_party.wait_for_refresh(None);

    rcv_party.show_unspent_colorings("receiver run 2 after refresh 1");
    party.show_unspent_colorings("sender run 2 no refresh");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_4.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    rcv_party.fail_transfers(None, true, true).unwrap();
    rcv_party.show_unspent_colorings("receiver run 2 after fail");
    party.show_unspent_colorings("sender run 2 after fail");
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_4.recipient_id,
        TransferStatus::Failed
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn waiting_safe_height() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_nia(None);

    // 1st transfer: wallet 1 > wallet 2
    let receive_data_1 = party_2.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
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
    // txid_1 has only one confirmation, so transfer parks in WaitingSafeHeight
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
            assignment: Assignment::Fungible(amount_2),
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

    // fail the receive transfer in WaitingSafeHeight
    assert!(party_2.fail_transfers_single(receive_data_2.batch_transfer_idx));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
}
