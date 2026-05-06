use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue NIA asset
    let asset = party.issue_asset_nia(None);

    // single transfer (issuance)
    let bak_info_before = party.db_backup_info();
    let transfer_list = party.list_transfers(Some(&asset.asset_id));
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.requested_assignment, None);
    assert_eq!(transfer.assignments, vec![Assignment::Fungible(AMOUNT)]);
    assert_eq!(transfer.status, TransferStatus::Settled);

    // new wallet
    let mut party = get_funded_party!();

    // issue CFA asset
    let asset = party.issue_asset_cfa(None, None);

    // single transfer (issuance)
    let transfer_list = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.requested_assignment, None,);
    assert_eq!(transfer.assignments, vec![Assignment::Fungible(AMOUNT)]);
    assert_eq!(transfer.status, TransferStatus::Settled);

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount * 2),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // multiple transfers (sender)
    let transfer_list = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfer_list.len(), 3);
    let transfer_send_1 = transfer_list
        .iter()
        .find(|t| {
            t.kind == TransferKind::Send
                && t.recipient_id == Some(receive_data_1.recipient_id.clone())
        })
        .unwrap();
    let transfer_send_2 = transfer_list
        .iter()
        .find(|t| {
            t.kind == TransferKind::Send
                && t.recipient_id == Some(receive_data_2.recipient_id.clone())
        })
        .unwrap();
    assert_eq!(
        transfer_send_1.requested_assignment,
        Some(Assignment::Fungible(amount))
    );
    assert_eq!(
        transfer_send_1.assignments,
        vec![Assignment::Fungible(AMOUNT - amount * 3)]
    );
    assert_eq!(
        transfer_send_2.requested_assignment,
        Some(Assignment::Fungible(amount * 2))
    );
    assert_eq!(
        transfer_send_2.assignments,
        vec![Assignment::Fungible(AMOUNT - amount * 3)]
    );
    assert_eq!(transfer_send_1.status, TransferStatus::WaitingCounterparty);
    assert_eq!(transfer_send_2.status, TransferStatus::WaitingCounterparty);
    assert_eq!(transfer_send_1.txid, Some(txid.clone()));
    assert_eq!(transfer_send_2.txid, Some(txid.clone()));

    // refresh once, so the asset appears on the receiver side
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // multiple transfers (receiver)
    let transfer_list_rcv = rcv_party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfer_list_rcv.len(), 2);
    let transfer_recv_blind = transfer_list_rcv
        .iter()
        .find(|t| t.kind == TransferKind::ReceiveBlind)
        .unwrap();
    let transfer_recv_witness = transfer_list_rcv
        .iter()
        .find(|t| t.kind == TransferKind::ReceiveWitness)
        .unwrap();
    assert_eq!(
        transfer_recv_blind.requested_assignment,
        Some(Assignment::Any)
    );
    assert_eq!(
        transfer_recv_blind.assignments,
        vec![Assignment::Fungible(amount)]
    );
    assert_eq!(
        transfer_recv_witness.requested_assignment,
        Some(Assignment::Any)
    );
    assert_eq!(
        transfer_recv_witness.assignments,
        vec![Assignment::Fungible(amount * 2)]
    );
    assert_eq!(
        transfer_recv_blind.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        transfer_recv_witness.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_recv_blind.txid, Some(txid.clone()));
    assert_eq!(transfer_recv_witness.txid, Some(txid.clone()));

    // refresh a second time to settle the transfers
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // check all transfers are now in status Settled
    let transfer_list = party.list_transfers(Some(&asset.asset_id));
    let transfer_list_rcv = rcv_party.list_transfers(Some(&asset.asset_id));
    assert!(
        transfer_list
            .iter()
            .all(|t| t.status == TransferStatus::Settled)
    );
    assert!(
        transfer_list_rcv
            .iter()
            .all(|t| t.status == TransferStatus::Settled)
    );
}

#[test]
#[parallel]
fn fail() {
    let party = offline_party!(get_test_wallet(false, None));

    // asset not found
    let result = party.list_transfers_result(Some("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
