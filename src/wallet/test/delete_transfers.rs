use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // return false if no transfer has changed
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 0);
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert!(!test_delete_transfers(&wallet, None, false));
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 0);

    // delete single transfer
    let receive_data = test_blind_receive(&mut wallet);
    test_fail_transfers_single(&mut wallet, online, receive_data.batch_transfer_idx);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_before = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 1);
    assert!(test_delete_transfers(
        &wallet,
        Some(receive_data.batch_transfer_idx),
        false
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let bak_info_after = txn.get_backup_info().unwrap().unwrap();
    txn.commit().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 0);

    // delete all Failed transfers
    let receive_data_1 = test_blind_receive(&mut wallet);
    let receive_data_2 = test_blind_receive(&mut wallet);
    let receive_data_3 = test_blind_receive(&mut wallet);
    test_fail_transfers_single(&mut wallet, online, receive_data_1.batch_transfer_idx);
    test_fail_transfers_single(&mut wallet, online, receive_data_2.batch_transfer_idx);
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
    show_unspent_colorings(&mut wallet, "run 1 before delete");
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 3);
    assert!(test_delete_transfers(&wallet, None, false));
    show_unspent_colorings(&mut wallet, "run 1 after delete");
    let txn = wallet.database().begin_transaction().unwrap();
    let transfers = txn.iter_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(transfers.len(), 1);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 1);

    // fail and delete remaining pending transfers
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 1);
    assert!(test_fail_transfers_single(
        &mut wallet,
        online,
        receive_data_3.batch_transfer_idx
    ));
    assert!(test_delete_transfers(
        &wallet,
        Some(receive_data_3.batch_transfer_idx),
        false
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let transfers = txn.iter_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(transfers.len(), 0);
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 0);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, online, None);

    // delete an initiated transfer with no RGB change
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = wallet
        .send_begin(
            online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let batch_transfer_idx = send_result.batch_transfer_idx.unwrap();
    test_fail_transfers_single(&mut wallet, online, batch_transfer_idx);
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 2);
    let txn = wallet.database().begin_transaction().unwrap();
    let txos_before = txn.iter_txos().unwrap();
    txn.commit().unwrap();
    assert_eq!(txos_before.len(), 5);
    let txn = wallet.database().begin_transaction().unwrap();
    let colorings_before = txn.iter_colorings().unwrap();
    txn.commit().unwrap();
    assert_eq!(colorings_before.len(), 2);
    assert!(test_delete_transfers(
        &wallet,
        Some(batch_transfer_idx),
        false
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 1);
    let txn = wallet.database().begin_transaction().unwrap();
    let txos_after = txn.iter_txos().unwrap();
    txn.commit().unwrap();
    assert_eq!(txos_after.len(), 5);
    let txn = wallet.database().begin_transaction().unwrap();
    let colorings_after = txn.iter_colorings().unwrap();
    txn.commit().unwrap();
    assert_eq!(colorings_after.len(), 1);

    // delete an initiated transfer with RGB change on bitcoin change UTXO
    let receive_data = test_blind_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT - 1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = wallet
        .send_begin(
            online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let batch_transfer_idx = send_result.batch_transfer_idx.unwrap();
    test_fail_transfers_single(&mut wallet, online, batch_transfer_idx);
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 2);
    let txn = wallet.database().begin_transaction().unwrap();
    let txos_before = txn.iter_txos().unwrap();
    txn.commit().unwrap();
    assert_eq!(txos_before.len(), 6);
    let txn = wallet.database().begin_transaction().unwrap();
    let colorings_before = txn.iter_colorings().unwrap();
    txn.commit().unwrap();
    assert_eq!(colorings_before.len(), 3);
    assert!(test_delete_transfers(
        &wallet,
        Some(batch_transfer_idx),
        false
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 1);
    let txn = wallet.database().begin_transaction().unwrap();
    let txos_after = txn.iter_txos().unwrap();
    txn.commit().unwrap();
    assert_eq!(txos_after.len(), 5);
    let txn = wallet.database().begin_transaction().unwrap();
    let colorings_after = txn.iter_colorings().unwrap();
    txn.commit().unwrap();
    assert_eq!(colorings_after.len(), 1);

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data_1 = test_blind_receive(&mut wallet);
    let receive_data_2 = wallet
        .blind_receive(
            Some(asset.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(test_fail_transfers_single(
        &mut wallet,
        online,
        receive_data_1.batch_transfer_idx
    ));
    assert!(test_fail_transfers_single(
        &mut wallet,
        online,
        receive_data_2.batch_transfer_idx
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
    show_unspent_colorings(&mut wallet, "run 2 before delete");
    assert!(test_delete_transfers(&wallet, None, true));
    show_unspent_colorings(&mut wallet, "run 2 after delete");
    let txn = wallet.database().begin_transaction().unwrap();
    let transfers = txn.iter_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(transfers.len(), 2);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_2.recipient_id,
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
    let (mut rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, online, None);
    let asset_id = asset.asset_id;

    // failed transfer can be deleted
    let receive_data_1 = test_blind_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&mut rcv_wallet_2);
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
    let send_result = test_send_result(&mut wallet, online, &recipient_map).unwrap();
    assert!(!send_result.txid.is_empty());
    test_fail_transfers_single(&mut wallet, online, send_result.batch_transfer_idx);
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_before = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_before.len(), 2);
    assert!(test_delete_transfers(
        &wallet,
        Some(send_result.batch_transfer_idx),
        false
    ));
    let txn = wallet.database().begin_transaction().unwrap();
    let batch_transfers_after = txn.iter_batch_transfers().unwrap();
    txn.commit().unwrap();
    assert_eq!(batch_transfers_after.len(), 1);
}

#[cfg(feature = "electrum")]
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
        test_delete_transfers_result(&wallet, Some(receive_data.batch_transfer_idx), false);
    assert!(matches!(result, Err(Error::CannotDeleteBatchTransfer)));

    // don't delete unknown transfer
    let result = test_delete_transfers_result(&wallet, Some(UNKNOWN_IDX), false);
    assert!(matches!(
        result,
        Err(Error::BatchTransferNotFound { idx }) if idx == UNKNOWN_IDX
    ));

    // issue
    let asset = test_issue_asset_nia(&mut wallet, online, None);
    show_unspent_colorings(&mut wallet, "after issuance");

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data = wallet
        .blind_receive(
            Some(asset.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    test_fail_transfers_all(&mut wallet, online);
    let result = test_delete_transfers_result(&wallet, Some(receive_data.batch_transfer_idx), true);
    assert!(matches!(result, Err(Error::CannotDeleteBatchTransfer)));
}
