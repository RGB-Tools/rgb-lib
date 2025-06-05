use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // return false if no transfer has changed
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(!test_delete_transfers(&wallet, None, false));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // delete single transfer
    let receive_data = test_blind_receive(&wallet);
    test_fail_transfers_single(&mut wallet, &online, receive_data.batch_transfer_idx);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data.recipient_id,
        TransferStatus::Failed
    ));
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(test_delete_transfers(
        &wallet,
        Some(receive_data.batch_transfer_idx),
        false
    ));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // delete all Failed transfers
    let receive_data_1 = test_blind_receive(&wallet);
    let receive_data_2 = test_blind_receive(&wallet);
    let receive_data_3 = test_blind_receive(&wallet);
    test_fail_transfers_single(&mut wallet, &online, receive_data_1.batch_transfer_idx);
    test_fail_transfers_single(&mut wallet, &online, receive_data_2.batch_transfer_idx);
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
    test_delete_transfers(&wallet, None, false);
    show_unspent_colorings(&mut wallet, "run 1 after delete");
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 1);
    assert!(check_test_transfer_status_recipient(
        &wallet,
        &receive_data_3.recipient_id,
        TransferStatus::WaitingCounterparty
    ));

    // fail and delete remaining pending transfers
    assert!(test_fail_transfers_single(
        &mut wallet,
        &online,
        receive_data_3.batch_transfer_idx
    ));
    assert!(test_delete_transfers(
        &wallet,
        Some(receive_data_3.batch_transfer_idx),
        false
    ));
    let transfers = wallet.database.iter_transfers().unwrap();
    assert_eq!(transfers.len(), 0);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data_1 = test_blind_receive(&wallet);
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
        &online,
        receive_data_1.batch_transfer_idx
    ));
    assert!(test_fail_transfers_single(
        &mut wallet,
        &online,
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
    let transfers = wallet.database.iter_transfers().unwrap();
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
    let (rcv_wallet_1, _rcv_online_1) = get_funded_wallet!();
    let (rcv_wallet_2, _rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id;

    // failed transfer can be deleted
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
    assert!(!send_result.txid.is_empty());
    test_fail_transfers_single(&mut wallet, &online, send_result.batch_transfer_idx);
    test_delete_transfers(&wallet, Some(send_result.batch_transfer_idx), false);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let receive_data = test_blind_receive(&wallet);

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
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
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
    test_fail_transfers_all(&mut wallet, &online);
    let result = test_delete_transfers_result(&wallet, Some(receive_data.batch_transfer_idx), true);
    assert!(matches!(result, Err(Error::CannotDeleteBatchTransfer)));
}
