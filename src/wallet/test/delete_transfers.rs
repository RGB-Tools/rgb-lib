use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // return false if no transfer has changed
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 0);
    let bak_info_before = party.db_backup_info();
    assert!(!party.delete_transfers(None, false));
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 0);

    // delete single transfer
    let receive_data = party.blind_receive();
    party.fail_transfers_single(receive_data.batch_transfer_idx);
    assert!(
        party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    let bak_info_before = party.db_backup_info();
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 1);
    assert!(party.delete_transfers(Some(receive_data.batch_transfer_idx), false));
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 0);

    // delete all Failed transfers
    let receive_data_1 = party.blind_receive();
    let receive_data_2 = party.blind_receive();
    let receive_data_3 = party.blind_receive();
    party.fail_transfers_single(receive_data_1.batch_transfer_idx);
    party.fail_transfers_single(receive_data_2.batch_transfer_idx);
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
    party.show_unspent_colorings("run 1 before delete");
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 3);
    assert!(party.delete_transfers(None, false));
    party.show_unspent_colorings("run 1 after delete");
    let transfers = party.db_transfers();
    assert_eq!(transfers.len(), 1);
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_3.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 1);

    // fail and delete remaining pending transfers
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 1);
    assert!(party.fail_transfers_single(receive_data_3.batch_transfer_idx));
    assert!(party.delete_transfers(Some(receive_data_3.batch_transfer_idx), false));
    let transfers = party.db_transfers();
    assert_eq!(transfers.len(), 0);
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 0);

    // issue
    let asset = party.issue_asset_nia(None);

    // delete an initiated transfer with no RGB change
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = party
        .wallet
        .send_begin(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let batch_transfer_idx = send_result.batch_transfer_idx.unwrap();
    party.fail_transfers_single(batch_transfer_idx);
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 2);
    let txos_before = party.db_txos();
    assert_eq!(txos_before.len(), 5);
    let colorings_before = party.db_colorings();
    assert_eq!(colorings_before.len(), 2);
    assert!(party.delete_transfers(Some(batch_transfer_idx), false));
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 1);
    let txos_after = party.db_txos();
    assert_eq!(txos_after.len(), 5);
    let colorings_after = party.db_colorings();
    assert_eq!(colorings_after.len(), 1);

    // delete an initiated transfer with RGB change on bitcoin change UTXO
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT - 1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = party
        .wallet
        .send_begin(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let batch_transfer_idx = send_result.batch_transfer_idx.unwrap();
    party.fail_transfers_single(batch_transfer_idx);
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 2);
    let txos_before = party.db_txos();
    assert_eq!(txos_before.len(), 6);
    let colorings_before = party.db_colorings();
    assert_eq!(colorings_before.len(), 3);
    assert!(party.delete_transfers(Some(batch_transfer_idx), false));
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 1);
    let txos_after = party.db_txos();
    assert_eq!(txos_after.len(), 5);
    let colorings_after = party.db_colorings();
    assert_eq!(colorings_after.len(), 1);

    // don't delete failed transfer with asset_id if no_asset_only is true
    let receive_data_1 = party.blind_receive();
    let receive_data_2 = party
        .wallet
        .blind_receive(
            Some(asset.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(party.fail_transfers_single(receive_data_1.batch_transfer_idx));
    assert!(party.fail_transfers_single(receive_data_2.batch_transfer_idx));
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::Failed
    ));
    assert!(party.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Failed
    ));
    party.show_unspent_colorings("run 2 before delete");
    assert!(party.delete_transfers(None, true));
    party.show_unspent_colorings("run 2 after delete");
    let transfers = party.db_transfers();
    assert_eq!(transfers.len(), 2);
    assert!(party.check_test_transfer_status_recipient(
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

    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);
    let asset_id = asset.asset_id;

    // failed transfer can be deleted
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
    assert!(!send_result.txid.is_empty());
    party.fail_transfers_single(send_result.batch_transfer_idx);
    let batch_transfers_before = party.db_batch_transfers();
    assert_eq!(batch_transfers_before.len(), 2);
    assert!(party.delete_transfers(Some(send_result.batch_transfer_idx), false));
    let batch_transfers_after = party.db_batch_transfers();
    assert_eq!(batch_transfers_after.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let mut party = get_funded_party!();

    let receive_data = party.blind_receive();

    // don't delete transfer not in Failed status
    assert!(
        !party.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );
    let result = party.delete_transfers_result(Some(receive_data.batch_transfer_idx), false);
    assert!(matches!(result, Err(Error::CannotDeleteBatchTransfer)));

    // don't delete unknown transfer
    let result = party.delete_transfers_result(Some(UNKNOWN_IDX), false);
    assert!(matches!(
        result,
        Err(Error::BatchTransferNotFound { idx }) if idx == UNKNOWN_IDX
    ));

    // issue
    let asset = party.issue_asset_nia(None);
    party.show_unspent_colorings("after issuance");

    // don't delete failed transfer with asset_id if no_asset_only is true
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
    party.fail_transfers_all();
    let result = party.delete_transfers_result(Some(receive_data.batch_transfer_idx), true);
    assert!(matches!(result, Err(Error::CannotDeleteBatchTransfer)));
}
