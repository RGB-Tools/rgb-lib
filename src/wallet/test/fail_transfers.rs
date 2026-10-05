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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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

    // === offline tests

    let mut offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let result = offline_party
        .wallet
        .fail_transfers(Online { id: 0 }, None, false, false);
    assert_matches!(result, Err(Error::Offline));

    // === online tests

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
            default_rcv_expiration(),
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

    // don't fail incoming transfer: waiting counterparty -> broadcast
    let result = rcv_party.fail_transfers(Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        rcv_party
            .check_test_transfer_status_recipient(&recipient_id, TransferStatus::WaitingBroadcast)
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

    // don't fail incoming transfer: waiting broadcast (fallible only after expiration)
    let result = rcv_party.fail_transfers(Some(batch_transfer_idx), false, false);
    assert!(matches!(result, Err(Error::CannotFailBatchTransfer)));
    assert!(
        rcv_party
            .check_test_transfer_status_recipient(&recipient_id, TransferStatus::WaitingBroadcast)
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
            default_send_expiration(),
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
        TransferStatus::WaitingBroadcast
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
            (now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64,
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

/// failing a witness receive must not leave its pending witness state behind: a payment to its
/// address is then an ordinary (spendable) UTXO, instead of being stuck as pending witness
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_receive_cleanup() {
    initialize();

    let fast_sync = SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FastSync,
    };
    let full_sync = SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FullSync,
    };
    // an RGB transfer to a witness receive, broadcast right away (as a donation) since the receive
    // is not going to ACK it
    let send_rgb_to = |party: &mut SinglesigParty, asset_id: &str, recipient_id: &str| {
        let recipient_map = HashMap::from([(
            asset_id.to_string(),
            vec![Recipient {
                assignment: Assignment::Fungible(66),
                recipient_id: recipient_id.to_string(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        )]);
        party
            .wallet
            .send(
                party.online,
                recipient_map,
                true,
                FEE_RATE,
                MIN_CONFIRMATIONS,
                default_send_expiration(),
            )
            .unwrap()
            .txid
    };
    // the BTC part of a transfer to a failed witness receive is an ordinary UTXO, while its RGB part
    // is ignored
    let assert_only_btc_received =
        |rcv_party: &mut SinglesigParty, receive_data: &ReceiveData, txid: &str| {
            let txos = rcv_party.db_txos();
            assert_eq!(txos.len(), 1);
            assert_eq!(txos[0].txid, txid);
            assert!(txos[0].exists);
            assert!(!txos[0].pending_witness);
            let unspents = rcv_party.list_unspents(false);
            let unspent = unspents
                .iter()
                .find(|u| u.utxo.outpoint.txid == txid)
                .unwrap();
            assert!(unspent.rgb_allocations.is_empty());
            assert!(rcv_party.list_assets(&[]).nia.unwrap().is_empty());
            // a refresh doesn't bring the failed receive back
            rcv_party.refresh_result(None, &[]).unwrap();
            assert!(rcv_party.check_test_transfer_status_recipient(
                &receive_data.recipient_id,
                TransferStatus::Failed
            ));
        };

    let mut party = get_funded_party!();
    let asset = party.issue_asset_nia(None);

    //
    // payment after the fail, within the grace time: detected by fast sync as an ordinary UTXO
    //

    let mut rcv_party = get_empty_party!();

    // a witness receive registers its script as pending witness
    let receive_data = rcv_party.witness_receive();
    assert_eq!(rcv_party.db_pending_witness_scripts().len(), 1);

    // fail the receive: the script keeps being watched for a while, in case the counterparty
    // broadcasts anyway
    assert!(rcv_party.fail_transfers_single(receive_data.batch_transfer_idx));
    assert!(
        rcv_party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    rcv_party.sync(fast_sync);
    assert_eq!(rcv_party.db_pending_witness_scripts().len(), 1);

    // an RGB transfer to the receive, broadcast anyway: fast sync detects its BTC part as an
    // ordinary UTXO, its RGB part is ignored
    let txid = send_rgb_to(&mut party, &asset.asset_id, &receive_data.recipient_id);
    mine(false);
    // settle the donation, so that its change can be sent again
    party.refresh_result(None, &[]).unwrap();
    rcv_party.sync(fast_sync);
    assert_only_btc_received(&mut rcv_party, &receive_data, &txid);
    assert!(rcv_party.db_pending_witness_scripts().is_empty());
    // the UTXO is usable (e.g. to allocate a blind receive)
    rcv_party.blind_receive();

    //
    // payment after the fail, past the grace time: only a full sync detects it
    //

    let mut rcv_party = get_empty_party!();
    let receive_data = rcv_party.witness_receive();
    assert!(rcv_party.fail_transfers_single(receive_data.batch_transfer_idx));

    // the script is dropped by the next sync once the grace time has elapsed
    rcv_party
        .wallet
        .go_online(OnlineOptions {
            failed_witness_receive_grace_secs: 0,
            ..test_go_online_options(None)
        })
        .unwrap();
    rcv_party.sync(fast_sync);
    assert!(rcv_party.db_pending_witness_scripts().is_empty());

    // an RGB transfer to the receive is not detected by fast sync anymore, a full sync is needed
    let txid = send_rgb_to(&mut party, &asset.asset_id, &receive_data.recipient_id);
    mine(false);
    party.refresh_result(None, &[]).unwrap();
    rcv_party.sync(fast_sync);
    assert!(rcv_party.db_txos().is_empty());
    rcv_party.sync(full_sync);
    assert_only_btc_received(&mut rcv_party, &receive_data, &txid);
    rcv_party.blind_receive();

    //
    // payment before the fail (donation broadcast, then consignment refused): the TXO has been
    // detected as pending witness and must be unflagged by the fail
    //

    let mut rcv_party = get_empty_party!();
    let _guard = stop_mining();
    // an out-of-band receive, so that the consignment can be provided (and refused) manually
    let receive_data = rcv_party
        .wallet
        .witness_receive(
            None,
            Assignment::Any,
            default_rcv_expiration(),
            vec![],
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(66),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: vec![],
        }],
    )]);
    let OperationResult { txid, .. } = party
        .wallet
        .send(
            party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            default_send_expiration(),
        )
        .unwrap();
    rcv_party.sync(fast_sync);
    let txos = rcv_party.db_txos();
    assert_eq!(txos.len(), 1);
    assert_eq!(txos[0].txid, txid);
    assert!(txos[0].pending_witness);
    assert!(rcv_party.db_pending_witness_scripts().is_empty());
    let outpoint = txos[0].outpoint();
    // the UTXO is not usable while flagged as pending witness
    let result = rcv_party.wallet.blind_receive(
        None,
        Assignment::Any,
        default_rcv_expiration(),
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // refuse the consignment (invalid against its own genesis after a schema swap): the TXO
    // becomes an ordinary UTXO
    let consignment_path = party
        .wallet
        .get_send_consignment_path(&asset.asset_id, &txid)
        .to_string_lossy()
        .to_string();
    let mut consignment = RgbTransfer::load_file(&consignment_path).unwrap();
    consignment.schema = CollectibleFungibleAsset::schema();
    let invalid_file = tempfile::NamedTempFile::with_prefix("witness_receive_cleanup::").unwrap();
    consignment.save_file(invalid_file.path()).unwrap();
    let refreshed = rcv_party
        .wallet
        .provide_out_of_band_consignment(
            rcv_party.online,
            invalid_file.path().to_string_lossy().to_string(),
            vec![],
        )
        .unwrap();
    assert_eq!(
        refreshed.into_values().next().unwrap().updated_status,
        Some(TransferStatus::Failed)
    );
    assert!(!rcv_party.db_txo(&outpoint).unwrap().pending_witness);
    assert_only_btc_received(&mut rcv_party, &receive_data, &txid);
    rcv_party.blind_receive();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ack_failure() {
    initialize();

    let amount: u64 = 66;
    let expiration_secs: u64 = 20;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(Some(&[AMOUNT]));

    let (server, _mock) = failing_ack_proxy();
    let failing_ack_endpoint = format!("rpc://{}/json-rpc", server.host_with_port());

    // send to 2 recipients, with transfers expiring soon
    let expiration = (now().unix_timestamp() + expiration_secs as i64) as u64;
    let receive_data_1 = rcv_party
        .blind_receive_with_endpoints(Some(expiration), vec![failing_ack_endpoint.clone()]);
    let receive_data_2 = rcv_party
        .blind_receive_with_endpoints(Some(expiration), vec![failing_ack_endpoint.clone()]);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: vec![failing_ack_endpoint.clone()],
            },
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: vec![failing_ack_endpoint],
            },
        ],
    )]);
    let send_result = party.send(recipient_map, FEE_RATE, Some(expiration));
    assert!(!send_result.txid.is_empty());

    // refreshing the receives fails to post the ACK
    let refresh_res = rcv_party.refresh_result(None, &[]).unwrap();
    for batch_transfer_idx in [
        receive_data_1.batch_transfer_idx,
        receive_data_2.batch_transfer_idx,
    ] {
        let refreshed = refresh_res.get(&batch_transfer_idx).unwrap();
        assert!(refreshed.updated_status.is_none());
        assert_matches!(refreshed.failure, Some(Error::Proxy { .. }));
    }
    // refreshing the send fails to get the ACK
    let refresh_res = party.refresh_result(None, &[]).unwrap();
    let refreshed = refresh_res.get(&send_result.batch_transfer_idx).unwrap();
    assert!(refreshed.updated_status.is_none());
    assert_matches!(refreshed.failure, Some(Error::Proxy { .. }));

    // before expiration the error could be transient, so the transfers cannot be failed
    let result = rcv_party.fail_transfers(Some(receive_data_1.batch_transfer_idx), false, false);
    assert_matches!(result, Err(Error::Proxy { .. }));
    let result = party.fail_transfers(Some(send_result.batch_transfer_idx), false, false);
    assert_matches!(result, Err(Error::Proxy { .. }));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(
        party.check_test_transfer_status_sender(
            &send_result.txid,
            TransferStatus::WaitingCounterparty
        )
    );

    // wait for the transfers to expire
    let wait_secs = (expiration as i64 - now().unix_timestamp()).clamp(0, i64::MAX) as u64 + 2;
    std::thread::sleep(std::time::Duration::from_secs(wait_secs));

    // once expired, the transfers can be failed both explicitly and as expired ones
    assert!(rcv_party.fail_transfers_single(receive_data_1.batch_transfer_idx));
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(rcv_party.fail_transfers_all());
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
    assert!(party.fail_transfers_single(send_result.batch_transfer_idx));
    assert!(party.check_test_transfer_status_sender(&send_result.txid, TransferStatus::Failed));
}
