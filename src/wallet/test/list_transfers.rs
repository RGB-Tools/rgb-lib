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
    assert_eq!(transfer_recv_blind.status, TransferStatus::WaitingBroadcast);
    assert_eq!(
        transfer_recv_witness.status,
        TransferStatus::WaitingBroadcast
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

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn filters() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset_nia = party.issue_asset_nia(None);
    let asset_cfa = party.issue_asset_cfa(None, None);

    // one batch tx carrying transfers of both assets
    let receive_nia = rcv_party.blind_receive();
    let receive_cfa = rcv_party.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_nia.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_cfa.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // AnyOrNone + txid: the tx's transfers across all assets
    let by_txid =
        party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::Txid(txid.clone()));
    assert_eq!(by_txid.len(), 2);
    assert!(by_txid.iter().all(|t| t.txid == Some(txid.clone())));

    // Id + txid: intersection, restricted to the given asset
    let nia_by_txid = party.list_transfers_filtered(
        AssetFilter::Id(asset_nia.asset_id.clone()),
        TransferScope::Txid(txid.clone()),
    );
    assert_eq!(nia_by_txid.len(), 1);
    let expected: Vec<i32> = party
        .list_transfers(Some(&asset_nia.asset_id))
        .into_iter()
        .filter(|t| t.txid == Some(txid.clone()))
        .map(|t| t.idx)
        .collect();
    assert_eq!(vec![nia_by_txid[0].idx], expected);

    // AnyOrNone + no txid: the whole history (2 issuances + 2 sends)
    let all = party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::All);
    assert_eq!(all.len(), 4);

    // None: receiver's pending blind receives not yet tied to an asset
    let pending = rcv_party.list_transfers_filtered(AssetFilter::None, TransferScope::All);
    assert_eq!(pending.len(), 2);

    // extra receive that nothing is sent to, stays asset-less
    let receive_extra = rcv_party.blind_receive();

    // settle the batch so the change is spendable again
    rcv_party.wait_for_refresh(None);
    // the refresh tied the 2 receives to their assets, only the extra one stays asset-less
    let all_rcv = rcv_party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::All);
    assert_eq!(all_rcv.len(), 3);
    let pending = rcv_party.list_transfers_filtered(AssetFilter::None, TransferScope::All);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].recipient_id, Some(receive_extra.recipient_id));
    // receiver side: the tx's transfers span 2 batch transfers sharing the same txid
    let rcv_by_txid = rcv_party
        .list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::Txid(txid.clone()));
    assert_eq!(rcv_by_txid.len(), 2);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // Id + txid of a tx not carrying that asset: empty
    let receive_nia_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_nia_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party.send_retry(&recipient_map);
    assert!(
        party
            .list_transfers_filtered(
                AssetFilter::Id(asset_cfa.asset_id.clone()),
                TransferScope::Txid(txid_2.clone())
            )
            .is_empty()
    );

    // unknown txid: empty
    assert!(
        party
            .list_transfers_filtered(
                AssetFilter::AnyOrNone,
                TransferScope::Txid(FAKE_TXID.to_string())
            )
            .is_empty()
    );
}

#[test]
#[parallel]
fn fail() {
    let data_dir = PrivateDataDir::new();
    let party = offline_party!(data_dir.wallet(false, None));

    // asset not found
    let result = party.list_transfers_result(Some("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // asset not found also when a txid is given
    let result = party.list_transfers_filtered_result(
        AssetFilter::Id(s!("rgb1inexistent")),
        TransferScope::Txid(FAKE_TXID.to_string()),
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // asset not found also when a batch transfer index is given
    let result = party.list_transfers_filtered_result(
        AssetFilter::Id(s!("rgb1inexistent")),
        TransferScope::BatchTransfer(UNKNOWN_IDX),
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // batch transfer not found
    for asset_filter in [AssetFilter::AnyOrNone, AssetFilter::None] {
        let result = party.list_transfers_filtered_result(
            asset_filter,
            TransferScope::BatchTransfer(UNKNOWN_IDX),
        );
        assert!(matches!(
            result,
            Err(Error::BatchTransferNotFound { idx }) if idx == UNKNOWN_IDX
        ));
    }

    // unknown txid: empty
    let result = party.list_transfers_filtered_result(
        AssetFilter::AnyOrNone,
        TransferScope::Txid(FAKE_TXID.to_string()),
    );
    assert!(result.unwrap().is_empty());
}

// check that, for every scope and asset filter, the result is identical to the `All` result
// filtered by batch transfer index or txid
#[cfg(feature = "electrum")]
fn check_scopes_match_all(party: &impl OfflineSigParty, asset_filters: &[AssetFilter]) {
    for asset_filter in asset_filters {
        let all = party.list_transfers_filtered(asset_filter.clone(), TransferScope::All);
        let all_any = party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::All);
        let mut scopes: Vec<TransferScope> = vec![];
        for transfer in &all_any {
            let mut transfer_scopes =
                vec![TransferScope::BatchTransfer(transfer.batch_transfer_idx)];
            if let Some(txid) = &transfer.txid {
                transfer_scopes.push(TransferScope::Txid(txid.clone()));
            }
            for scope in transfer_scopes {
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
        }
        for scope in scopes {
            let expected: Vec<&Transfer> = all
                .iter()
                .filter(|t| match &scope {
                    TransferScope::All => true,
                    TransferScope::Txid(txid) => t.txid.as_ref() == Some(txid),
                    TransferScope::BatchTransfer(idx) => t.batch_transfer_idx == *idx,
                })
                .collect();
            let scoped = party.list_transfers_filtered(asset_filter.clone(), scope.clone());
            assert_eq!(
                format!("{:?}", scoped.iter().collect::<Vec<_>>()),
                format!("{expected:?}"),
                "mismatch for filter {asset_filter:?} and scope {scope:?}"
            );
        }
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn scopes() {
    initialize();

    let amount: u64 = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset_nia = party.issue_asset_nia(None);
    let asset_cfa = party.issue_asset_cfa(None, None);
    let filters = [
        AssetFilter::AnyOrNone,
        AssetFilter::None,
        AssetFilter::Id(asset_nia.asset_id.clone()),
        AssetFilter::Id(asset_cfa.asset_id.clone()),
    ];

    // one batch transfer carrying transfers of both assets
    let receive_nia = rcv_party.blind_receive();
    let receive_cfa = rcv_party.witness_receive();
    // extra receive that nothing is sent to, stays asset-less
    let _receive_extra = rcv_party.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_nia.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_cfa.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // sender: the batch transfer contains a transfer for each asset
    let send_batch_idx = party
        .list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::Txid(txid.clone()))
        .first()
        .unwrap()
        .batch_transfer_idx;
    let send_batch = TransferScope::BatchTransfer(send_batch_idx);
    let by_batch = party.list_transfers_filtered(AssetFilter::AnyOrNone, send_batch.clone());
    assert_eq!(by_batch.len(), 2);
    assert!(by_batch.iter().all(|t| t.txid == Some(txid.clone())));
    for asset_id in [&asset_nia.asset_id, &asset_cfa.asset_id] {
        let by_batch_asset =
            party.list_transfers_filtered(AssetFilter::Id(asset_id.clone()), send_batch.clone());
        assert_eq!(by_batch_asset.len(), 1);
    }
    assert!(
        party
            .list_transfers_filtered(AssetFilter::None, send_batch)
            .is_empty()
    );
    // receiver: pending receives have no txid yet, each one is its own batch transfer
    let rcv_nia = rcv_party.list_transfers_filtered(
        AssetFilter::AnyOrNone,
        TransferScope::BatchTransfer(receive_nia.batch_transfer_idx),
    );
    assert_eq!(rcv_nia.len(), 1);
    assert_eq!(
        rcv_nia[0].recipient_id,
        Some(receive_nia.recipient_id.clone())
    );
    check_scopes_match_all(&party, &filters);
    check_scopes_match_all(&rcv_party, &filters[..2]);

    // receiver: after a refresh the txid is shared by 2 batch transfers
    rcv_party.wait_for_refresh(None);
    let rcv_by_txid = rcv_party
        .list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::Txid(txid.clone()));
    assert_eq!(rcv_by_txid.len(), 2);
    let mut rcv_batch_idxs: Vec<i32> = rcv_by_txid.iter().map(|t| t.batch_transfer_idx).collect();
    rcv_batch_idxs.sort();
    let mut expected_batch_idxs = vec![
        receive_nia.batch_transfer_idx,
        receive_cfa.batch_transfer_idx,
    ];
    expected_batch_idxs.sort();
    assert_eq!(rcv_batch_idxs, expected_batch_idxs);
    let rcv_by_txid_nia = rcv_party.list_transfers_filtered(
        AssetFilter::Id(asset_nia.asset_id.clone()),
        TransferScope::Txid(txid.clone()),
    );
    assert_eq!(rcv_by_txid_nia.len(), 1);
    assert_eq!(
        rcv_by_txid_nia[0].batch_transfer_idx,
        receive_nia.batch_transfer_idx
    );
    check_scopes_match_all(&party, &filters);
    check_scopes_match_all(&rcv_party, &filters);

    // settle and check again
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    check_scopes_match_all(&party, &filters);
    check_scopes_match_all(&rcv_party, &filters);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn scopes_extra_allocations() {
    initialize();

    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();
    party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // both assets are allocated to the same UTXO
    let asset = party.issue_asset_nia(None);
    let asset_extra = party.issue_asset_cfa(Some(&[AMOUNT * 2]), Some(FILE_STR.to_string()));
    let filters = [
        AssetFilter::AnyOrNone,
        AssetFilter::None,
        AssetFilter::Id(asset.asset_id.clone()),
        AssetFilter::Id(asset_extra.asset_id.clone()),
    ];

    // sending one asset also moves the other one, with a non-user-driven asset transfer
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
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
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let (_, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    assert!(
        asset_transfers
            .iter()
            .any(|a| a.asset_id == Some(asset_extra.asset_id.clone()) && !a.user_driven)
    );

    // only the user-driven transfer is listed for the batch transfer and the txid
    let by_txid =
        party.list_transfers_filtered(AssetFilter::AnyOrNone, TransferScope::Txid(txid.clone()));
    assert_eq!(by_txid.len(), 1);
    let batch_scope = TransferScope::BatchTransfer(by_txid[0].batch_transfer_idx);
    assert_eq!(
        party
            .list_transfers_filtered(AssetFilter::AnyOrNone, batch_scope.clone())
            .len(),
        1
    );
    assert!(
        party
            .list_transfers_filtered(AssetFilter::Id(asset_extra.asset_id.clone()), batch_scope)
            .is_empty()
    );
    check_scopes_match_all(&party, &filters);

    // settle and check again
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    check_scopes_match_all(&party, &filters);
}
