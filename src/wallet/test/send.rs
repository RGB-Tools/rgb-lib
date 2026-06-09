use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;
    let expiration_secs = 60i64;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);

    // send
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let expiration_timestamp = (now().unix_timestamp() + expiration_secs) as u64;
    let bak_info_before = party.db_backup_info();
    let operation_result = party.send(recipient_map, FEE_RATE, Some(expiration_timestamp));
    let bak_info_after = party.db_backup_info();
    let txid = operation_result.txid;
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(!txid.is_empty());
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        transfer_data.expiration_timestamp,
        Some(expiration_timestamp as i64)
    );

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, asset_transfer) = party.get_test_transfer_data(&transfer);

    // ack is None
    assert_eq!(rcv_transfer.ack, None);
    assert_eq!(transfer.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );
    // recipient_id is set
    assert_eq!(
        rcv_transfer.recipient_id,
        Some(receive_data.recipient_id.clone())
    );
    assert_eq!(
        transfer.recipient_id,
        Some(receive_data.recipient_id.clone())
    );
    // incoming
    assert!(rcv_transfer.incoming);
    assert!(!transfer.incoming);

    // change_utxo is set only for the sender
    assert!(rcv_transfer_data.change_utxo.is_none());
    assert!(transfer_data.change_utxo.is_some());
    // create and update timestamps are the same
    assert_eq!(rcv_transfer_data.created_at, rcv_transfer_data.updated_at);
    // transfer is incoming for receiver and outgoing for sender
    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveBlind);
    assert_eq!(transfer_data.kind, TransferKind::Send);
    // transfers start in WaitingCounterparty status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);
    // txid is set only for the sender
    assert_eq!(rcv_transfer_data.txid, None);
    assert_eq!(transfer_data.txid, Some(txid.clone()));
    // received UTXO is set only for the receiver
    assert!(rcv_transfer_data.receive_utxo.is_some());
    assert!(transfer_data.receive_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer.asset_id.is_none());
    assert_eq!(asset_transfer.asset_id, Some(asset.asset_id.clone()));
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer.user_driven);
    assert!(asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(Some(&asset.asset_id));
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);

    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
    // ack is now true on the sender side
    assert_eq!(transfer.ack, Some(true));
    // amount is now set on the receiver side
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount)]
    );
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer.asset_id, Some(asset.asset_id.clone()));
    // update timestamp has been updated
    let rcv_updated_at = rcv_transfer_data.updated_at;
    let updated_at = transfer_data.updated_at;
    assert!(rcv_updated_at > rcv_transfer_data.created_at);
    assert!(updated_at > transfer_data.created_at);

    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let nia_assets = rcv_assets.nia.unwrap();
    let cfa_assets = rcv_assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(cfa_assets.len(), 0);
    let rcv_asset = nia_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(rcv_asset.ticker, TICKER);
    assert_eq!(rcv_asset.name, NAME);
    assert_eq!(rcv_asset.precision, PRECISION);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: amount,
            spendable: 0,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    // update timestamp has been updated
    assert!(rcv_transfer_data.updated_at > rcv_updated_at);
    assert!(transfer_data.updated_at > updated_at);

    // change is unspent once transfer is Settled
    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo);
    assert!(change_unspent.is_some());

    // send, ignoring rgbhttpjsonrpc transport endpoints with non-compliant APIs or unsupported
    // protocol version
    let transport_endpoints = vec![
        format!("rpc://{PROXY_HOST_MOD_API}"),
        format!("rpc://{PROXY_HOST_MOD_PROTO}"),
        format!("rpc://{PROXY_HOST}"),
    ];
    let receive_data_api_proto =
        rcv_party.blind_receive_with_endpoints(None, transport_endpoints.clone());
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_api_proto.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let unspents = party.list_unspents(false);
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), 3);
    let mut tte_data_iter = tte_data.iter();
    let (ce_0, ce_1, ce_2) = (
        &tte_data_iter.next().unwrap(),
        &tte_data_iter.next().unwrap(),
        &tte_data_iter.next().unwrap(),
    );
    assert_eq!(ce_0.1.endpoint, PROXY_URL_MOD_API);
    assert_eq!(ce_1.1.endpoint, PROXY_URL_MOD_PROTO);
    assert_eq!(ce_2.1.endpoint, PROXY_URL);
    assert!(!ce_0.0.used);
    assert!(!ce_1.0.used);
    assert!(ce_2.0.used);
    let proxy_client_mod_api = get_proxy_client(Some(PROXY_URL_MOD_API));
    let consignment = proxy_client_mod_api
        .get_consignment(&receive_data_api_proto.recipient_id)
        .unwrap();
    assert!(consignment.error.is_some());
    let proxy_client_mod_proto = get_proxy_client(Some(PROXY_URL_MOD_PROTO));
    let consignment = proxy_client_mod_proto
        .get_consignment(&receive_data_api_proto.recipient_id)
        .unwrap();
    assert!(consignment.error.is_some());
    let proxy_client = get_proxy_client(Some(PROXY_URL));
    let consignment = proxy_client
        .get_consignment(&receive_data_api_proto.recipient_id)
        .unwrap();
    assert!(consignment.result.is_some());
    // settle transfer
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_api_proto.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = party.list_unspents(false);
    let unspents_color_count_after = unspents.iter().filter(|u| u.utxo.colorable).count();
    assert_eq!(unspents_color_count_after, unspents_color_count_before);

    // send, ignoring invalid or unreachable rpc transport endpoints and using a fee
    // rate that requires additional inputs be added to cover fee costs
    let transport_endpoints = vec![
        format!("rpc://{PROXY_HOST_MOD_PROTO}"),
        format!("rpc://127.6.6.6:7777/json-rpc"),
        format!("rpc://{PROXY_HOST}"),
    ];
    let receive_data_invalid_unreachable = rcv_party.blind_receive_with_endpoints(
        None,
        transport_endpoints.clone().into_iter().skip(1).collect(),
    );
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_invalid_unreachable.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let unspents = party.list_unspents(false);
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = party.send(recipient_map, 7, None).txid;
    assert!(!txid.is_empty());
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), 3);
    let mut tte_data_iter = tte_data.iter();
    let (ce_0, ce_1, ce_2) = (
        &tte_data_iter.next().unwrap(),
        &tte_data_iter.next().unwrap(),
        &tte_data_iter.next().unwrap(),
    );
    assert_eq!(ce_0.1.endpoint, PROXY_URL_MOD_PROTO);
    assert_eq!(ce_1.1.endpoint, "http://127.6.6.6:7777/json-rpc");
    assert_eq!(ce_2.1.endpoint, PROXY_URL);
    assert!(!ce_0.0.used);
    assert!(!ce_1.0.used);
    assert!(ce_2.0.used);
    let consignment = proxy_client_mod_proto
        .get_consignment(&receive_data_invalid_unreachable.recipient_id)
        .unwrap();
    assert!(consignment.error.is_some());
    let consignment = proxy_client
        .get_consignment(&receive_data_invalid_unreachable.recipient_id)
        .unwrap();
    assert!(consignment.result.is_some());
    // settle transfer
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = party.list_unspents(false);
    let unspents_color_count_after = unspents.iter().filter(|u| u.utxo.colorable).count();
    assert_eq!(unspents_color_count_after, unspents_color_count_before - 2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn spend_all() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();
    party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // issue
    let asset = party.issue_asset_nia(None);
    let asset_extra = party.issue_asset_cfa(Some(&[AMOUNT * 2]), Some(FILE_STR.to_string()));

    // check both assets are allocated to the same UTXO
    let unspents = party.list_unspents(true);
    let unspents_with_rgb_allocations: Vec<Unspent> = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert_eq!(unspents_with_rgb_allocations.len(), 1);
    let allocation_asset_ids: Vec<String> = unspents_with_rgb_allocations
        .first()
        .unwrap()
        .rgb_allocations
        .clone()
        .into_iter()
        .map(|a| a.asset_id.unwrap_or_else(|| s!("")))
        .collect();
    assert!(allocation_asset_ids.contains(&asset.asset_id));
    assert!(allocation_asset_ids.contains(&asset_extra.asset_id));

    // send
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

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfers, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = party.get_test_transfer_data(transfer);
    assert_eq!(asset_transfers.len(), 2);
    let asset_transfer = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset.asset_id.clone()))
        .unwrap();
    let asset_extra_asset_transfer = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset_extra.asset_id.clone()))
        .unwrap();

    // change_utxo is not set (sender has no asset change)
    assert!(rcv_transfer_data.change_utxo.is_none());
    assert!(transfer_data.change_utxo.is_none());
    // transfers start in WaitingCounterparty status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // asset transfers are user-driven on both sides
    assert!(rcv_asset_transfer.user_driven);
    assert!(asset_transfer.user_driven);
    // asset_extra asset transfer is not user driven
    assert!(!asset_extra_asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(Some(&asset.asset_id));
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = party.get_test_transfer_data(transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = party.get_test_transfer_data(transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check the completely spent asset doesn't show up in unspents anymore
    let unspents = party.list_unspents(true);
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset.asset_id.clone()))
    });
    assert!(!found);
    // check the extra asset shows up in unspents
    let unspents = party.list_unspents(true);
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset_extra.asset_id.clone()))
    });
    assert!(found);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_twice_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // transfer 1 checks
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_1);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(AMOUNT - amount_1)]
    );
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_1)
    );

    //
    // 2nd transfer
    //

    // send
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // transfer 2 checks
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_2);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount_2)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(AMOUNT - amount_1 - amount_2)]
    );
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_1 - amount_2)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_extra_success() {
    initialize();

    let supply_nia = AMOUNT;
    let supply_cfa = AMOUNT * 2;
    let amount_1: u64 = 66;
    let amount_2: u64 = 22;
    let amount_3: u64 = 7;
    let amount_4: u64 = 3;
    let amount_5: u64 = amount_1 + amount_2;

    // wallets
    let mut party_1 = get_funded_noutxo_party!();
    let mut party_2 = get_funded_noutxo_party!();

    // start with 1 UTXO only so blind receives all use the same one
    party_1.create_utxos(true, Some(1), None, FEE_RATE, None);
    party_2.create_utxos(true, Some(1), None, FEE_RATE, None);

    // issue
    let asset_nia = party_1.issue_asset_nia(None);
    let asset_cfa = party_1.issue_asset_cfa(Some(&[supply_cfa]), None);

    // check both assets are allocated to the same UTXO
    let unspents = party_1.list_unspents(true);
    let unspents_with_rgb_allocations: Vec<Unspent> = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert_eq!(unspents_with_rgb_allocations.len(), 1);
    let allocation_asset_ids: Vec<String> = unspents_with_rgb_allocations
        .first()
        .unwrap()
        .rgb_allocations
        .clone()
        .into_iter()
        .map(|a| a.asset_id.unwrap_or_else(|| s!("")))
        .collect();
    assert!(allocation_asset_ids.contains(&asset_nia.asset_id));
    assert!(allocation_asset_ids.contains(&asset_cfa.asset_id));
    party_1.show_unspent_colorings("wallet 1 after issuance");

    //
    // 1st transfer, asset_nia: wallet 1 > wallet 2
    //

    // send
    println!("\n=== send 1");
    let receive_data_1 = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());
    party_1.show_unspent_colorings("wallet 1 after send 1, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // transfer 1 checks
    let transfers_w1 = party_1.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = party_2.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w1.requested_assignment,
        Some(Assignment::Fungible(amount_1))
    );
    assert_eq!(
        transfer_w1.assignments,
        vec![Assignment::Fungible(supply_nia - amount_1)]
    );
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w2.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, supply_nia - amount_1);
    assert_eq!(balance_cfa_w1.settled, supply_cfa);
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    assert_eq!(balance_nia_w2.settled, amount_1);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = party_1.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let ca_a1 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert_eq!(
        ca_a1.assignment,
        Assignment::Fungible(supply_nia - amount_1)
    );
    assert_eq!(ca_a1.asset_id, Some(asset_nia.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.assignment, Assignment::Fungible(supply_cfa));
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);

    //
    // 2nd transfer, asset_nia: wallet 1 > wallet 2 (re-using the same recipient UTXO)
    //

    let receive_data_1b = party_2.blind_receive();
    println!("\n=== send 2");
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_1b.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_1.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());
    party_1.show_unspent_colorings("wallet 1 after send 2, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // transfer 2 checks
    let transfers_w1 = party_1.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = party_2.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w1.requested_assignment,
        Some(Assignment::Fungible(amount_2))
    );
    assert_eq!(
        transfer_w1.assignments,
        vec![Assignment::Fungible(supply_nia - amount_1 - amount_2)]
    );
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w2.assignments,
        vec![Assignment::Fungible(amount_2)]
    );
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, supply_nia - amount_1 - amount_2);
    assert_eq!(balance_cfa_w1.settled, supply_cfa);
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    assert_eq!(balance_nia_w2.settled, amount_1 + amount_2);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = party_1.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let ca_a1 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert_eq!(
        ca_a1.assignment,
        Assignment::Fungible(supply_nia - amount_1 - amount_2)
    );
    assert_eq!(ca_a1.asset_id, Some(asset_nia.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.assignment, Assignment::Fungible(supply_cfa));
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);
    // recipient allocations
    let unspents = party_2.list_unspents(true);
    let allocations: Vec<&RgbAllocation> =
        unspents.iter().flat_map(|u| &u.rgb_allocations).collect();
    let a_a1: Vec<&&RgbAllocation> = allocations
        .iter()
        .filter(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .collect();
    assert_eq!(a_a1.len(), 2);

    //
    // 3rd transfer, asset_cfa (extra in previous sends): wallet 1 > wallet 2
    //

    party_1.create_utxos(true, Some(2), None, FEE_RATE, Some(1));

    // send
    let receive_data_2 = party_2.blind_receive();
    println!("\n=== send 3");
    let recipient_map = HashMap::from([(
        asset_cfa.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_3),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = party_1.send_retry(&recipient_map);
    assert!(!txid_3.is_empty());
    party_1.show_unspent_colorings("wallet 1 after send 3, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // transfer 3 checks
    let transfers_w2 = party_2.list_transfers(Some(&asset_cfa.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = party_1.list_transfers(Some(&asset_cfa.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w1.requested_assignment,
        Some(Assignment::Fungible(amount_3))
    );
    assert_eq!(
        transfer_w1.assignments,
        vec![Assignment::Fungible(supply_cfa - amount_3)]
    );
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w2.assignments,
        vec![Assignment::Fungible(amount_3)]
    );
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, supply_nia - amount_1 - amount_2);
    assert_eq!(balance_cfa_w1.settled, supply_cfa - amount_3);
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w2 = party_2.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w2.settled, amount_1 + amount_2);
    assert_eq!(balance_cfa_w2.settled, amount_3);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = party_1.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let ca_a1 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert_eq!(
        ca_a1.assignment,
        Assignment::Fungible(supply_nia - amount_1 - amount_2)
    );
    assert_eq!(ca_a1.asset_id, Some(asset_nia.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(
        ca_a2.assignment,
        Assignment::Fungible(supply_cfa - amount_3)
    );
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);

    party_2.show_unspent_colorings("wallet 2 after send 3, Settled");

    //
    // 4th transfer, asset_cfa (2 asset_nia extra transitions): wallet 2 > wallet 1
    //

    let receive_data_4 = party_1.blind_receive();
    println!("\n=== send 4");
    let recipient_map = HashMap::from([(
        asset_cfa.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_4),
            recipient_id: receive_data_4.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_4 = party_2.send_retry(&recipient_map);
    assert!(!txid_4.is_empty());
    party_2.show_unspent_colorings("wallet 2 after send 4, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    // transfer 4 checks
    let transfers_w2 = party_2.list_transfers(Some(&asset_cfa.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = party_1.list_transfers(Some(&asset_cfa.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    // transfers data
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w2.requested_assignment,
        Some(Assignment::Fungible(amount_4))
    );
    assert_eq!(
        transfer_w2.assignments,
        vec![Assignment::Fungible(amount_3 - amount_4)]
    );
    assert_eq!(transfer_w2.kind, TransferKind::Send);
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w1.assignments,
        vec![Assignment::Fungible(amount_4)]
    );
    assert_eq!(transfer_w1.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, supply_nia - amount_1 - amount_2);
    assert_eq!(balance_cfa_w1.settled, supply_cfa - amount_3 + amount_4);
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w2 = party_2.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w2.settled, amount_1 + amount_2);
    assert_eq!(balance_cfa_w2.settled, amount_3 - amount_4);
    // sender change
    let change_utxo = transfer_w2.change_utxo.as_ref().unwrap();
    let unspents = party_2.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    println!("change_unspent {change_unspent:?}");
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 3);
    let ca_a1: Vec<&RgbAllocation> = change_allocations
        .iter()
        .filter(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .collect();
    assert_eq!(ca_a1.len(), 2);
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert!(
        ca_a1
            .iter()
            .any(|a| if let Assignment::Fungible(amt) = a.assignment {
                amt == amount_1
            } else {
                false
            })
    );
    assert!(
        ca_a1
            .iter()
            .any(|a| if let Assignment::Fungible(amt) = a.assignment {
                amt == amount_2
            } else {
                false
            })
    );
    assert!(
        ca_a1
            .iter()
            .all(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
    );
    assert!(ca_a1.iter().all(|a| a.settled));
    assert_eq!(ca_a2.assignment, Assignment::Fungible(amount_3 - amount_4));
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);

    //
    // 5th transfer, asset_nia (merging 2 allocations, no change): wallet 2 > wallet 1
    //

    let receive_data_5 = party_1.blind_receive();
    println!("\n=== send 5");
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_5),
            recipient_id: receive_data_5.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_5 = party_2.send_retry(&recipient_map);
    assert!(!txid_5.is_empty());
    party_2.show_unspent_colorings("wallet 2 after send 5, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    // transfer 5 checks
    let transfers_w2 = party_2.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = party_1.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    // transfers data
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w2.requested_assignment,
        Some(Assignment::Fungible(amount_5))
    );
    assert_eq!(transfer_w2.assignments, vec![]);
    assert_eq!(transfer_w2.kind, TransferKind::Send);
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w1.assignments,
        vec![Assignment::Fungible(amount_5)]
    );
    assert_eq!(transfer_w1.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, supply_nia);
    assert_eq!(balance_cfa_w1.settled, supply_cfa - amount_3 + amount_4);
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w2 = party_2.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w2.settled, 0);
    assert_eq!(balance_cfa_w2.settled, amount_3 - amount_4);

    //
    // 6th transfer, asset_nia (send all, 2 allocations, no change): wallet 1 > wallet 2
    //

    let receive_data_6 = party_2.blind_receive();
    println!("\n=== send 6");
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(supply_nia),
            recipient_id: receive_data_6.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_6 = party_1.send_retry(&recipient_map);
    assert!(!txid_6.is_empty());
    party_1.show_unspent_colorings("wallet 1 after send 6, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // transfer 6 checks
    let transfers_w1 = party_1.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = party_2.list_transfers(Some(&asset_nia.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(
        transfer_w1.requested_assignment,
        Some(Assignment::Fungible(supply_nia))
    );
    assert_eq!(transfer_w1.assignments, vec![]);
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_w2.assignments,
        vec![Assignment::Fungible(supply_nia)]
    );
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // check balances
    let balance_nia_w2 = party_2.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w2 = party_2.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w2.settled, supply_nia);
    assert_eq!(balance_cfa_w2.settled, amount_3 - amount_4);
    let balance_nia_w1 = party_1.get_asset_balance(&asset_nia.asset_id);
    let balance_cfa_w1 = party_1.get_asset_balance(&asset_cfa.asset_id);
    assert_eq!(balance_nia_w1.settled, 0);
    assert_eq!(balance_cfa_w1.settled, supply_cfa - amount_3 + amount_4);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_success() {
    initialize();

    let amount_1a: u64 = 66;
    let amount_1b: u64 = 33;
    let amount_2a: u64 = 7;
    let amount_2b: u64 = 4;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    // issue
    let asset_nia = party_1.issue_asset_nia(None);
    let asset_cfa = party_1.issue_asset_cfa(Some(&[AMOUNT * 2]), Some(FILE_STR.to_string()));

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_a20 = party_2.blind_receive();
    let receive_data_a25 = party_2.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_1a),
                recipient_id: receive_data_a20.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_1b),
                recipient_id: receive_data_a25.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_1 = party_1.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // transfer 1 checks
    let (transfers_w1, _, _) = party_1.get_test_transfers_sender(&txid_1);
    let transfers_for_asset_nia = transfers_w1.get(&asset_nia.asset_id).unwrap();
    let transfers_for_asset_cfa = transfers_w1.get(&asset_cfa.asset_id).unwrap();
    assert_eq!(transfers_for_asset_nia.len(), 1);
    assert_eq!(transfers_for_asset_cfa.len(), 1);
    let transfer_w1a = transfers_for_asset_nia.first().unwrap();
    let transfer_w1b = transfers_for_asset_cfa.first().unwrap();
    let transfer_w2a = party_2.get_test_transfer_recipient(&receive_data_a20.recipient_id);
    let transfer_w2b = party_2.get_test_transfer_recipient(&receive_data_a25.recipient_id);
    let (transfer_data_w1a, _) = party_1.get_test_transfer_data(transfer_w1a);
    let (transfer_data_w1b, _) = party_1.get_test_transfer_data(transfer_w1b);
    let (transfer_data_w2a, _) = party_2.get_test_transfer_data(&transfer_w2a);
    let (transfer_data_w2b, _) = party_2.get_test_transfer_data(&transfer_w2b);
    assert_eq!(
        transfer_w1a.requested_assignment,
        Some(Assignment::Fungible(amount_1a))
    );
    assert_eq!(
        transfer_w1b.requested_assignment,
        Some(Assignment::Fungible(amount_1b))
    );
    assert_eq!(
        transfer_data_w2a.assignments,
        vec![Assignment::Fungible(amount_1a)]
    );
    assert_eq!(
        transfer_data_w2b.assignments,
        vec![Assignment::Fungible(amount_1b)]
    );
    assert_eq!(transfer_data_w1a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w1b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);

    let unspents = party_1.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w1a.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    let change_allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .unwrap();
    let change_allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert_eq!(change_allocations.len(), 2);
    assert_eq!(
        change_allocation_a.assignment,
        Assignment::Fungible(AMOUNT - amount_1a)
    );
    assert_eq!(
        change_allocation_b.assignment,
        Assignment::Fungible(AMOUNT * 2 - amount_1b)
    );

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_b20 = party_3.blind_receive();
    let receive_data_b25 = party_3.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_2a),
                recipient_id: receive_data_b20.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_2b),
                recipient_id: receive_data_b25.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_2 = party_2.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    // transfer 2 checks
    let (transfers_w2, _, _) = party_2.get_test_transfers_sender(&txid_2);
    let transfers_for_asset_nia = transfers_w2.get(&asset_nia.asset_id).unwrap();
    let transfers_for_asset_cfa = transfers_w2.get(&asset_cfa.asset_id).unwrap();
    assert_eq!(transfers_for_asset_nia.len(), 1);
    assert_eq!(transfers_for_asset_cfa.len(), 1);
    let transfer_w2a = transfers_for_asset_nia.first().unwrap();
    let transfer_w2b = transfers_for_asset_cfa.first().unwrap();
    let transfer_w3a = party_3.get_test_transfer_recipient(&receive_data_b20.recipient_id);
    let transfer_w3b = party_3.get_test_transfer_recipient(&receive_data_b25.recipient_id);
    let (transfer_data_w2a, _) = party_2.get_test_transfer_data(transfer_w2a);
    let (transfer_data_w2b, _) = party_2.get_test_transfer_data(transfer_w2b);
    let (transfer_data_w3a, _) = party_3.get_test_transfer_data(&transfer_w3a);
    let (transfer_data_w3b, _) = party_3.get_test_transfer_data(&transfer_w3b);
    assert_eq!(
        transfer_w2a.requested_assignment,
        Some(Assignment::Fungible(amount_2a))
    );
    assert_eq!(
        transfer_w2b.requested_assignment,
        Some(Assignment::Fungible(amount_2b))
    );
    assert_eq!(
        transfer_data_w3a.assignments,
        vec![Assignment::Fungible(amount_2a)]
    );
    assert_eq!(
        transfer_data_w3b.assignments,
        vec![Assignment::Fungible(amount_2b)]
    );
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3b.status, TransferStatus::Settled);

    let unspents = party_2.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w2a.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    let change_allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_nia.asset_id.clone()))
        .unwrap();
    let change_allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_cfa.asset_id.clone()))
        .unwrap();
    assert_eq!(change_allocations.len(), 2);
    assert_eq!(
        change_allocation_a.assignment,
        Assignment::Fungible(amount_1a - amount_2a)
    );
    assert_eq!(
        change_allocation_b.assignment,
        Assignment::Fungible(amount_1b - amount_2b)
    );

    // check CFA asset has the correct media after being received
    let cfa_assets = party_3.list_assets(&[AssetSchema::Cfa]).cfa.unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let recv_asset = cfa_assets.first().unwrap();
    let dst_path = recv_asset.media.as_ref().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_uda_success() {
    initialize();

    let amount_1: u64 = 1;
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str, FILE_STR]);
    assert!(party_1.db_asset(&asset.asset_id).media_idx.is_none());

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_1 = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));

    // transfer 1 checks
    let (transfer_w1, _, _) = party_1.get_test_transfer_sender(&txid_1);
    let transfer_w2 = party_2.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (transfer_data_w1, _) = party_1.get_test_transfer_data(&transfer_w1);
    let (transfer_data_w2, _) = party_2.get_test_transfer_data(&transfer_w2);
    assert_eq!(transfer_data_w1.assignments, vec![]);
    assert_eq!(transfer_data_w2.assignments, vec![Assignment::NonFungible]);
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_2 = party_3.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::NonFungible,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_2.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_3.wait_for_refresh(None);
    assert!(party_3.db_asset(&asset.asset_id).media_idx.is_none());
    party_2.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));

    // transfer 2 checks
    let transfer_w3 = party_3.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (transfer_w2, _, _) = party_2.get_test_transfer_sender(&txid_2);
    let (transfer_data_w3, _) = party_3.get_test_transfer_data(&transfer_w3);
    let (transfer_data_w2, _) = party_2.get_test_transfer_data(&transfer_w2);
    assert_eq!(transfer_data_w3.assignments, vec![Assignment::NonFungible]);
    assert_eq!(transfer_data_w2.assignments, vec![]);
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    // check asset has been received correctly
    let uda_assets = party_3.list_assets(&[AssetSchema::Uda]).uda.unwrap();
    assert_eq!(uda_assets.len(), 1);
    let recv_asset = uda_assets.first().unwrap();
    assert_eq!(recv_asset.asset_id, asset.asset_id);
    assert_eq!(recv_asset.name, NAME.to_string());
    assert_eq!(recv_asset.details, Some(DETAILS.to_string()));
    assert_eq!(recv_asset.precision, PRECISION);
    assert_eq!(
        recv_asset.balance,
        Balance {
            settled: amount_1,
            future: amount_1,
            spendable: amount_1,
        }
    );
    let token = recv_asset.token.as_ref().unwrap();
    // check media mime-type
    let media = token.media.as_ref().unwrap();
    assert_eq!(media.mime, "text/plain");
    // check media data matches
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    // check attachments
    let media = token.attachments.get(&0).unwrap();
    assert_eq!(media.mime, "image/png");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(image_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    let media = token.attachments.get(&1).unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_cfa_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_cfa(None, Some(FILE_STR.to_string()));

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_1 = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));

    // transfer 1 checks
    let (transfer_w1, _, _) = party_1.get_test_transfer_sender(&txid_1);
    let transfer_w2 = party_2.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (transfer_data_w1, _) = party_1.get_test_transfer_data(&transfer_w1);
    let (transfer_data_w2, _) = party_2.get_test_transfer_data(&transfer_w2);
    assert_eq!(
        transfer_data_w1.assignments,
        vec![Assignment::Fungible(AMOUNT - amount_1)]
    );
    assert_eq!(
        transfer_data_w2.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = party_1.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w1.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_1)
    );

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_2 = party_3.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_2.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));

    // transfer 2 checks
    let transfer_w3 = party_3.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (transfer_w2, _, _) = party_2.get_test_transfer_sender(&txid_2);
    let (transfer_data_w3, _) = party_3.get_test_transfer_data(&transfer_w3);
    let (transfer_data_w2, _) = party_2.get_test_transfer_data(&transfer_w2);
    assert_eq!(
        transfer_data_w3.assignments,
        vec![Assignment::Fungible(amount_2)]
    );
    assert_eq!(
        transfer_data_w2.assignments,
        vec![Assignment::Fungible(amount_1 - amount_2)]
    );
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = party_2.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w2.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(amount_1 - amount_2)
    );
    // check asset has been received correctly
    let cfa_assets = party_3.list_assets(&[AssetSchema::Cfa]).cfa.unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let recv_asset = cfa_assets.first().unwrap();
    assert_eq!(recv_asset.asset_id, asset.asset_id);
    assert_eq!(recv_asset.name, NAME.to_string());
    assert_eq!(recv_asset.details, Some(DETAILS.to_string()));
    assert_eq!(recv_asset.precision, PRECISION);
    assert_eq!(
        recv_asset.balance,
        Balance {
            settled: amount_2,
            future: amount_2,
            spendable: amount_2,
        }
    );
    // check media mime-type
    let media = recv_asset.media.as_ref().unwrap();
    assert_eq!(media.mime, "text/plain");
    // check media data matches
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(FILE_STR)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_digest = hash_bytes_hex(&src_bytes[..]);
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn receive_multiple_same_asset_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_1),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount_2),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(asset_transfers.len(), 1);
    assert_eq!(transfers.len(), 1);
    let asset_transfer = asset_transfers.first().unwrap();
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_1.recipient_id.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_2.recipient_id.clone()))
        .unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    // ack is None
    assert_eq!(rcv_transfer_1.ack, None);
    assert_eq!(rcv_transfer_2.ack, None);
    assert_eq!(transfer_1.ack, None);
    assert_eq!(transfer_2.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer_1.requested_assignment, Some(Assignment::Any));
    assert_eq!(rcv_transfer_2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_1.requested_assignment,
        Some(Assignment::Fungible(amount_1))
    );
    assert_eq!(
        transfer_2.requested_assignment,
        Some(Assignment::Fungible(amount_2))
    );
    // recipient_id is set
    assert_eq!(
        rcv_transfer_1.recipient_id,
        Some(receive_data_1.recipient_id.clone())
    );
    assert_eq!(
        rcv_transfer_2.recipient_id,
        Some(receive_data_2.recipient_id.clone())
    );
    assert_eq!(
        transfer_1.recipient_id,
        Some(receive_data_1.recipient_id.clone())
    );
    assert_eq!(
        transfer_2.recipient_id,
        Some(receive_data_2.recipient_id.clone())
    );

    // change_utxo is set only for the sender and it's the same for all transfers
    assert!(rcv_transfer_data_1.change_utxo.is_none());
    assert!(rcv_transfer_data_2.change_utxo.is_none());
    assert!(transfer_data_1.change_utxo.is_some());
    assert!(transfer_data_2.change_utxo.is_some());
    assert_eq!(transfer_data_1.change_utxo, transfer_data_2.change_utxo);
    // create and update timestamps are the same
    assert_eq!(
        rcv_transfer_data_1.created_at,
        rcv_transfer_data_1.updated_at
    );
    assert_eq!(
        rcv_transfer_data_2.created_at,
        rcv_transfer_data_2.updated_at
    );
    // transfer is incoming for receiver and outgoing for sender
    assert_eq!(rcv_transfer_data_1.kind, TransferKind::ReceiveBlind);
    assert_eq!(rcv_transfer_data_2.kind, TransferKind::ReceiveBlind);
    assert_eq!(transfer_data_1.kind, TransferKind::Send);
    assert_eq!(transfer_data_2.kind, TransferKind::Send);
    // transfers start in WaitingCounterparty status
    assert_eq!(
        rcv_transfer_data_1.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(
        rcv_transfer_data_2.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data_1.status, TransferStatus::WaitingCounterparty);
    assert_eq!(transfer_data_2.status, TransferStatus::WaitingCounterparty);
    // txid is set only for the sender
    assert_eq!(rcv_transfer_data_1.txid, None);
    assert_eq!(rcv_transfer_data_2.txid, None);
    assert_eq!(transfer_data_1.txid, Some(txid.clone()));
    assert_eq!(transfer_data_2.txid, Some(txid.clone()));
    // received UTXO is set only for the receiver
    assert!(rcv_transfer_data_1.receive_utxo.is_some());
    assert!(rcv_transfer_data_2.receive_utxo.is_some());
    assert!(transfer_data_1.receive_utxo.is_none());
    assert!(transfer_data_2.receive_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer_1.asset_id.is_none());
    assert!(rcv_asset_transfer_1.asset_id.is_none());
    assert!(rcv_asset_transfer_2.asset_id.is_none());
    assert!(rcv_asset_transfer_2.asset_id.is_none());
    assert_eq!(asset_transfer.asset_id, Some(asset.asset_id.clone()));
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer_1.user_driven);
    assert!(rcv_asset_transfer_2.user_driven);
    assert!(asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(transfers.len(), 1);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_1.recipient_id.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_2.recipient_id.clone()))
        .unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    assert_eq!(
        rcv_transfer_data_1.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_transfer_data_2.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data_1.status, TransferStatus::WaitingConfirmations);
    assert_eq!(transfer_data_2.status, TransferStatus::WaitingConfirmations);
    // ack is now true on the sender side
    assert_eq!(transfer_1.ack, Some(true));
    assert_eq!(transfer_2.ack, Some(true));
    // amount is now set on the receiver side
    assert_eq!(
        rcv_transfer_data_1.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(
        rcv_transfer_data_2.assignments,
        vec![Assignment::Fungible(amount_2)]
    );
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer_1.asset_id, Some(asset.asset_id.clone()));
    assert_eq!(rcv_asset_transfer_2.asset_id, Some(asset.asset_id.clone()));
    // update timestamp has been updated
    let rcv_updated_at_1 = rcv_transfer_data_1.updated_at;
    let rcv_updated_at_2 = rcv_transfer_data_2.updated_at;
    let updated_at_1 = transfer_data_1.updated_at;
    let updated_at_2 = transfer_data_2.updated_at;
    assert!(rcv_updated_at_1 > rcv_transfer_data_1.created_at);
    assert!(rcv_updated_at_2 > rcv_transfer_data_2.created_at);
    assert!(updated_at_1 > transfer_data_1.created_at);
    assert!(updated_at_2 > transfer_data_2.created_at);

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, _) = rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, _) = rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(transfers.len(), 1);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_1.recipient_id.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.recipient_id == Some(receive_data_2.recipient_id.clone()))
        .unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    assert_eq!(rcv_transfer_data_1.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer_data_2.status, TransferStatus::Settled);
    assert_eq!(transfer_data_1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_2.status, TransferStatus::Settled);
    // update timestamp has been updated
    assert!(rcv_transfer_data_1.updated_at > rcv_updated_at_1);
    assert!(rcv_transfer_data_2.updated_at > rcv_updated_at_2);
    assert!(transfer_data_1.updated_at > updated_at_1);
    assert!(transfer_data_2.updated_at > updated_at_1);

    // change is unspent once transfer is Settled
    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_1.change_utxo);
    assert!(change_unspent.is_some());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn receive_multiple_different_assets_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset_1 = party.issue_asset_nia(None);
    let asset_2 = party
        .wallet
        .issue_asset_cfa(
            s!("NAME2"),
            Some(DETAILS.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
        )
        .unwrap();

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_1),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_2.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount_2),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(asset_transfers.len(), 2);
    assert_eq!(transfers.len(), 2);
    let asset_transfer_1 = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        .unwrap();
    let asset_transfer_2 = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset_2.asset_id.clone()))
        .unwrap();
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    // ack is None
    assert_eq!(rcv_transfer_1.ack, None);
    assert_eq!(rcv_transfer_2.ack, None);
    assert_eq!(transfer_1.ack, None);
    assert_eq!(transfer_2.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer_1.requested_assignment, Some(Assignment::Any));
    assert_eq!(rcv_transfer_2.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer_1.requested_assignment,
        Some(Assignment::Fungible(amount_1))
    );
    assert_eq!(
        transfer_2.requested_assignment,
        Some(Assignment::Fungible(amount_2))
    );
    // recipient_id is set
    assert_eq!(
        rcv_transfer_1.recipient_id,
        Some(receive_data_1.recipient_id.clone())
    );
    assert_eq!(
        rcv_transfer_2.recipient_id,
        Some(receive_data_2.recipient_id.clone())
    );
    assert_eq!(
        transfer_1.recipient_id,
        Some(receive_data_1.recipient_id.clone())
    );
    assert_eq!(
        transfer_2.recipient_id,
        Some(receive_data_2.recipient_id.clone())
    );

    // change_utxo is set only for the sender and it's the same for all transfers
    assert!(rcv_transfer_data_1.change_utxo.is_none());
    assert!(rcv_transfer_data_2.change_utxo.is_none());
    assert!(transfer_data_1.change_utxo.is_some());
    assert!(transfer_data_2.change_utxo.is_some());
    assert_eq!(transfer_data_1.change_utxo, transfer_data_2.change_utxo);
    // create and update timestamps are the same
    assert_eq!(
        rcv_transfer_data_1.created_at,
        rcv_transfer_data_1.updated_at
    );
    assert_eq!(
        rcv_transfer_data_2.created_at,
        rcv_transfer_data_2.updated_at
    );
    // transfers are incoming for receiver and outgoing for sender
    assert_eq!(rcv_transfer_data_1.kind, TransferKind::ReceiveBlind);
    assert_eq!(rcv_transfer_data_2.kind, TransferKind::ReceiveBlind);
    assert_eq!(transfer_data_1.kind, TransferKind::Send);
    assert_eq!(transfer_data_2.kind, TransferKind::Send);
    // transfers start in WaitingCounterparty status
    assert_eq!(
        rcv_transfer_data_1.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(
        rcv_transfer_data_2.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data_1.status, TransferStatus::WaitingCounterparty);
    assert_eq!(transfer_data_2.status, TransferStatus::WaitingCounterparty);
    // txid is set only for the sender
    assert_eq!(rcv_transfer_data_1.txid, None);
    assert_eq!(rcv_transfer_data_2.txid, None);
    assert_eq!(transfer_data_1.txid, Some(txid.clone()));
    assert_eq!(transfer_data_2.txid, Some(txid.clone()));
    // received UTXO is set only for the receiver
    assert!(rcv_transfer_data_1.receive_utxo.is_some());
    assert!(rcv_transfer_data_2.receive_utxo.is_some());
    assert!(transfer_data_1.receive_utxo.is_none());
    assert!(transfer_data_2.receive_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer_1.asset_id.is_none());
    assert!(rcv_asset_transfer_1.asset_id.is_none());
    assert!(rcv_asset_transfer_2.asset_id.is_none());
    assert!(rcv_asset_transfer_2.asset_id.is_none());
    assert_eq!(asset_transfer_1.asset_id, Some(asset_1.asset_id.clone()));
    assert_eq!(asset_transfer_2.asset_id, Some(asset_2.asset_id.clone()));
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer_1.user_driven);
    assert!(rcv_asset_transfer_2.user_driven);
    assert!(asset_transfer_1.user_driven);
    assert!(asset_transfer_2.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_1.asset_id));

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    assert_eq!(
        rcv_transfer_data_1.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_transfer_data_2.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data_1.status, TransferStatus::WaitingConfirmations);
    assert_eq!(transfer_data_2.status, TransferStatus::WaitingConfirmations);
    // ack is now true on the sender side
    assert_eq!(transfer_1.ack, Some(true));
    assert_eq!(transfer_2.ack, Some(true));
    // amount is now set on the receiver side
    assert_eq!(
        rcv_transfer_data_1.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(
        rcv_transfer_data_2.assignments,
        vec![Assignment::Fungible(amount_2)]
    );
    // asset id is now set on the receiver side
    assert_eq!(
        rcv_asset_transfer_1.asset_id,
        Some(asset_1.asset_id.clone())
    );
    assert_eq!(
        rcv_asset_transfer_2.asset_id,
        Some(asset_2.asset_id.clone())
    );
    // update timestamp has been updated
    let rcv_updated_at_1 = rcv_transfer_data_1.updated_at;
    let rcv_updated_at_2 = rcv_transfer_data_2.updated_at;
    let updated_at_1 = transfer_data_1.updated_at;
    let updated_at_2 = transfer_data_2.updated_at;
    assert!(rcv_updated_at_1 > rcv_transfer_data_1.created_at);
    assert!(rcv_updated_at_2 > rcv_transfer_data_2.created_at);
    assert!(updated_at_1 > transfer_data_1.created_at);
    assert!(updated_at_2 > transfer_data_2.created_at);

    // assets have been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let nia_assets = rcv_assets.nia.unwrap();
    let cfa_assets = rcv_assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(cfa_assets.len(), 1);
    let rcv_asset_nia = nia_assets.last().unwrap();
    assert_eq!(rcv_asset_nia.asset_id, asset_1.asset_id);
    assert_eq!(rcv_asset_nia.ticker, TICKER);
    assert_eq!(rcv_asset_nia.name, NAME);
    assert_eq!(rcv_asset_nia.precision, PRECISION);
    assert_eq!(
        rcv_asset_nia.balance,
        Balance {
            settled: 0,
            future: amount_1,
            spendable: 0,
        }
    );
    let rcv_asset_cfa = cfa_assets.last().unwrap();
    assert_eq!(rcv_asset_cfa.asset_id, asset_2.asset_id);
    assert_eq!(rcv_asset_cfa.name, s!("NAME2"));
    assert_eq!(rcv_asset_cfa.details, Some(DETAILS.to_string()));
    assert_eq!(rcv_asset_cfa.precision, PRECISION);
    assert_eq!(
        rcv_asset_cfa.balance,
        Balance {
            settled: 0,
            future: amount_2,
            spendable: 0,
        }
    );
    assert_eq!(rcv_asset_cfa.media, None);

    // transfers progress to status Settled after tx mining + refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_1.asset_id));

    let rcv_transfer_1 = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let rcv_transfer_2 = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data_1, _) = rcv_party.get_test_transfer_data(&rcv_transfer_1);
    let (rcv_transfer_data_2, _) = rcv_party.get_test_transfer_data(&rcv_transfer_2);
    let (transfers, _, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let (transfer_data_1, _) = party.get_test_transfer_data(transfer_1);
    let (transfer_data_2, _) = party.get_test_transfer_data(transfer_2);

    assert_eq!(rcv_transfer_data_1.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer_data_2.status, TransferStatus::Settled);
    assert_eq!(transfer_data_1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_2.status, TransferStatus::Settled);
    // update timestamp has been updated
    assert!(rcv_transfer_data_1.updated_at > rcv_updated_at_1);
    assert!(rcv_transfer_data_2.updated_at > rcv_updated_at_2);
    assert!(transfer_data_1.updated_at > updated_at_1);
    assert!(transfer_data_2.updated_at > updated_at_1);

    // change is unspent once transfer is Settled
    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_1.change_utxo);
    assert!(change_unspent.is_some());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn batch_donation_success() {
    initialize();

    let amount_a1 = 11;
    let amount_a2 = 12;
    let amount_b1 = 25;
    let amount_b2 = 22;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset_a = party.issue_asset_nia(None);
    let asset_b = party.issue_asset_nia(None);
    let _asset_c = party.issue_asset_nia(None);

    party.show_unspent_colorings("after issuances");

    // check each assets is allocated to a different UTXO
    let unspents = party.list_unspents(true);
    let unspents_with_rgb_allocations = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty());
    assert_eq!(unspents_with_rgb_allocations.count(), 3);

    // blind
    let receive_data_a1 = rcv_party_1.blind_receive();
    let receive_data_a2 = rcv_party_2.blind_receive();
    let receive_data_b1 = rcv_party_1.blind_receive();
    let receive_data_b2 = rcv_party_2.blind_receive();

    // send multiple assets to multiple recipients
    let recipient_map = HashMap::from([
        (
            asset_a.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount_a1),
                    recipient_id: receive_data_a1.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_a2),
                    recipient_id: receive_data_a2.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            asset_b.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount_b1),
                    recipient_id: receive_data_b1.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_b2),
                    recipient_id: receive_data_b2.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
    ]);
    let txid = party
        .wallet
        .send(
            party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    party.show_unspent_colorings("after send");

    // check change UTXO has all the expected allocations
    let transfers_a = party.list_transfers(Some(&asset_a.asset_id));
    let transfer_a = transfers_a.last().unwrap();
    let change_utxo = transfer_a.change_utxo.as_ref().unwrap();
    let unspents = party.list_unspents(false);
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_a.asset_id.clone()));
    let allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_b.asset_id.clone()));
    assert_eq!(
        allocation_a.unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_a1 - amount_a2)
    );
    assert_eq!(
        allocation_b.unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_b1 - amount_b2)
    );

    // take receiver transfers from WaitingCounterparty to Settled
    // (send_batch doesn't wait for recipient ACKs and proceeds to broadcast)
    rcv_party_1.wait_for_refresh(None);
    rcv_party_2.wait_for_refresh(None);
    rcv_party_1.list_transfers(Some(&asset_a.asset_id));
    rcv_party_1.list_transfers(Some(&asset_b.asset_id));
    rcv_party_2.list_transfers(Some(&asset_a.asset_id));
    rcv_party_2.list_transfers(Some(&asset_b.asset_id));
    mine(false);
    rcv_party_1.wait_for_refresh(None);
    rcv_party_2.wait_for_refresh(None);
    rcv_party_1.list_transfers(Some(&asset_a.asset_id));
    rcv_party_1.list_transfers(Some(&asset_b.asset_id));
    rcv_party_2.list_transfers(Some(&asset_a.asset_id));
    rcv_party_2.list_transfers(Some(&asset_b.asset_id));

    party.show_unspent_colorings("after send, settled");
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_failed_blinded_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue asset
    let asset = party.issue_asset_nia(None);

    // 1st transfer
    let receive_data =
        rcv_party.blind_receive_asset_expiry(None, Some((now().unix_timestamp() + 60) as u64));
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = party.send_result(&recipient_map).unwrap();
    assert!(!send_result.txid.is_empty());

    // try to send again and check the asset is not spendable
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());

    // fail transfer so asset allocation can be spent again
    party.fail_transfers_single(send_result.batch_transfer_idx);

    // 2nd transfer using the same blinded UTXO
    let result = party.send_result(&recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDAlreadyUsed)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send with donation set to false
    let receive_data_1 = rcv_party_1.blind_receive();
    let receive_data_2 = rcv_party_2.blind_receive();
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
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // all transfers are in WaitingCounterparty status
    assert!(rcv_party_1.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(rcv_party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingCounterparty));

    // ack from recipient 1 > its transfer status changes to WaitingConfirmations
    rcv_party_1.wait_for_refresh(None);
    assert!(rcv_party_1.check_test_transfer_status_recipient(
        &receive_data_1.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingCounterparty));

    // ack from recipient 2 > its transfer status changes to WaitingConfirmations
    rcv_party_2.wait_for_refresh(None);
    assert!(rcv_party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // now sender can broadcast and move on to WaitingConfirmations
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingConfirmations));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn nack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send with donation set to false
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // transfers are in WaitingCounterparty status
    assert!(rcv_party.check_test_transfer_status_recipient(
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::WaitingCounterparty));

    // manually NACK the transfer (consignment is valid so refreshing receiver would yield an ACK)
    let proxy_client = get_proxy_client(None);
    proxy_client
        .post_ack(&receive_data.recipient_id, false)
        .unwrap();

    // refreshing sender transfer now has it fail
    party.wait_for_refresh(Some(&asset.asset_id));
    assert!(party.check_test_transfer_status_sender(&txid, TransferStatus::Failed));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_change_on_pending_send() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 32;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();
    party.create_utxos(true, Some(3), None, FEE_RATE, None);

    // issue 1 + get its UTXO
    let asset_1 = party.issue_asset_nia(None);
    let unspents = party.list_unspents(false);
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // issue 2
    let asset_2 = party.issue_asset_nia(Some(&[AMOUNT * 2]));

    party.show_unspent_colorings("before 1st send");
    // send asset_1
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // send asset_2 (send_1 in WaitingCounterparty)
    party.show_unspent_colorings("before 2nd send");
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = party.send_result(&recipient_map).unwrap();
    let txid_2 = send_result.txid;
    assert!(!txid_2.is_empty());
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));
    // fail send asset_2
    party.fail_transfers_single(send_result.batch_transfer_idx);

    // progress send_1 to WaitingConfirmations
    party.show_unspent_colorings("before refresh");
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_1.asset_id));

    // send asset_2 (send_1 in WaitingConfirmations)
    party.show_unspent_colorings("before 3rd send");
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = party.send_retry(&recipient_map);
    assert!(!txid_3.is_empty());
    party.show_unspent_colorings("after 3rd send");
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));
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
    let result = offline_party.wallet.send(
        Online { id: 0 },
        HashMap::new(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
        None,
    );
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.send_begin(
        Online { id: 0 },
        HashMap::new(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
        None,
        false,
    );
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.send_end(Online { id: 0 }, s!(""));
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue asset
    let asset = party.issue_asset_nia(None);
    // blind
    let receive_data =
        rcv_party.blind_receive_asset_expiry(None, Some((now().unix_timestamp() + 60) as u64));

    // invalid recipient map: empty
    let recipient_map = HashMap::new();
    let result = party.send_begin_result(&recipient_map).unwrap_err();
    assert!(matches!(result, Error::InvalidRecipientMap));

    // invalid recipient map: no recipients
    let recipient_map = HashMap::from([(asset.asset_id.clone(), vec![])]);
    let result = party.send_begin_result(&recipient_map).unwrap_err();
    assert!(matches!(result, Error::InvalidRecipientMap));

    // invalid input (asset id)
    let recipient_map = HashMap::from([(
        s!("rgb1inexistent"),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // insufficient assets (amount too big)
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT + 1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let collection = AssignmentsCollection {
        fungible: AMOUNT,
        ..Default::default()
    };
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == collection);

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let msg = s!("must provide at least a transport endpoint");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: malformed
    let transport_endpoints = vec![s!("malformed")];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown:{PROXY_HOST}")];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: no valid endpoints (down, modified)
    let transport_endpoints = vec![
        format!("rpc://127.6.6.6:7777/json-rpc"),
        format!("rpc://{PROXY_HOST_MOD_PROTO}"),
    ];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let msg = s!("no valid transport endpoints");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: too many endpoints
    let transport_endpoints = vec![
        format!("rgbhttpjsonrpc:127.0.0.1:3000/json-rpc"),
        format!("rgbhttpjsonrpc:127.0.0.1:3001/json-rpc"),
        format!("rgbhttpjsonrpc:127.0.0.1:3002/json-rpc"),
        format!("rgbhttpjsonrpc:127.0.0.1:3003/json-rpc"),
    ];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let msg = s!("library supports at max 3 transport endpoints");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // fee min
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.wallet.send_begin(
        party.online,
        recipient_map.clone(),
        false,
        0,
        MIN_CONFIRMATIONS,
        None,
        false,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // fee overflow
    let result = party.wallet.send_begin(
        party.online,
        recipient_map.clone(),
        false,
        u64::MAX,
        MIN_CONFIRMATIONS,
        None,
        false,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));

    // duplicated recipient ID
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(AMOUNT / 2),
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(AMOUNT / 3),
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDDuplicated)));

    // amount 0
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(0),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::InvalidAmountZero)));

    // blinded with witness data
    let receive_data_blinded = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_blinded.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let details = "cannot provide witness data for a blinded recipient";
    assert!(matches!(result, Err(Error::InvalidRecipientData { details: m }) if m == details));

    // witness with no witness data
    let receive_data_witness = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_witness.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    let details = "missing witness data for a witness recipient";
    assert!(matches!(result, Err(Error::InvalidRecipientData { details: m }) if m == details));

    // output below dust limit
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data_witness.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 0,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // unsupported layer 1
    println!("setting MOCK_CHAIN_NET");
    MOCK_CHAIN_NET.replace(Some(ChainNet::LiquidTestnet));
    let receive_data_liquid = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_liquid.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::InvalidRecipientNetwork)));

    // transport endpoints: no valid endpoints
    let transport_endpoints = vec![format!("rpc://{PROXY_HOST_MOD_API}")];
    let receive_data_te = rcv_party.blind_receive_with_endpoints(None, transport_endpoints.clone());
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT / 2),
            recipient_id: receive_data_te.recipient_id,
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = party.send_result(&recipient_map);
    assert!(matches!(result, Err(Error::NoValidTransportEndpoint)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_incoming_transfer_fail() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_noutxo_party!();
    rcv_party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // issue
    let asset = party.issue_asset_nia(None);

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    party.show_unspent_colorings("sender after 1st send, settled");
    rcv_party.show_unspent_colorings("receiver after 1st send, settled");

    //
    // 2nd transfer
    //

    // add a blind to the same UTXO
    let _receive_data_2 = rcv_party.blind_receive();
    rcv_party.show_unspent_colorings("receiver after 2nd blind");

    // send from receiving wallet, 1st receive Settled, 2nd one still pending
    let receive_data = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // check input allocation is blocked by pending receive
    party.show_unspent_colorings("sender after 2nd send, WaitingCounterparty");
    rcv_party.show_unspent_colorings("receiver after 2nd send, WaitingCounterparty");
    let result = rcv_party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());

    // refresh on both wallets (no transfer status changes)
    // check input allocation is still blocked by pending receive
    assert!(!rcv_party.refresh_all());
    assert!(!party.refresh_asset(&asset.asset_id));
    let result = rcv_party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // issue asset
    let asset = party.issue_asset_nia(None);

    // 1st send
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party.show_unspent_colorings("sender after 1st send");

    // check change UTXO has exists = false and unspents list it
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    let unspents = party.list_unspents_with_sync(false);
    let change_unspent = unspents
        .iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    assert!(!change_unspent.utxo.exists);
    assert_eq!(unspents.len(), UTXO_NUM as usize + 2);
    assert_eq!(
        unspents
            .iter()
            .filter(|u| u.utxo.colorable && u.utxo.exists)
            .count(),
        UTXO_NUM as usize
    );
    assert_eq!(
        unspents
            .iter()
            .filter(|u| u.utxo.colorable && !u.utxo.exists)
            .count(),
        1
    );
    assert_eq!(unspents.iter().filter(|u| !u.utxo.colorable).count(), 1);

    // 2nd send (1st still pending)
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // check input allocation is blocked by pending send
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());

    // take transfer from WaitingCounterparty to WaitingConfirmations
    // check input allocation is still blocked by pending send
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_transfer_input_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();
    party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // issue
    let asset = party.issue_asset_nia(None);

    // blind with sender wallet to create a pending transfer
    let _receive_data = party.blind_receive();
    party.show_unspent_colorings("sender after blind");

    // send and check it fails as the issuance UTXO is "blocked" by the pending receive operation
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_result(&recipient_map);
    assert_matches!(result, Err(Error::InsufficientAssignments { asset_id: t, available: a }) if t == asset.asset_id && a == AssignmentsCollection::default());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn already_used_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue asset to 3 UTXOs
    let asset = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]));

    // 1st transfer
    let receive_data =
        rcv_party.blind_receive_asset_expiry(None, Some((now().unix_timestamp() + 60) as u64));
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // 2nd transfer using the same blinded UTXO
    let result = party.send_result(&recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDAlreadyUsed)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn cfa_extra_success() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create a single UTXO to issue assets on the same UTXO
    party.create_utxos(true, Some(1), None, FEE_RATE, None);

    // issue NIA
    let asset_nia = party.issue_asset_nia(None);

    // issue CFA
    let amount = 42;
    let _asset_cfa = party.issue_asset_cfa(Some(&[amount]), None);

    let receive_data = rcv_party.blind_receive();

    // send NIA
    let recipient_map = HashMap::from([(
        asset_nia.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    let (transfers, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(asset_transfers.len(), 2);
    assert_eq!(transfers.len(), 2);
    let asset_transfer_1 = &asset_transfers[0];
    assert!(asset_transfer_1.user_driven);
    let asset_transfer_2 = &asset_transfers[1];
    assert!(!asset_transfer_2.user_driven);
    let extra_colorings = party.db_colorings_filtered(asset_transfer_2.idx);
    let extra_coloring_input = &extra_colorings[0];
    let extra_coloring_change = &extra_colorings[1];
    assert_eq!(extra_coloring_input.r#type, ColoringType::Input);
    assert_eq!(
        extra_coloring_input.assignment,
        Assignment::Fungible(amount)
    );
    assert_eq!(extra_coloring_change.r#type, ColoringType::Change);
    assert_eq!(
        extra_coloring_change.assignment,
        Assignment::Fungible(amount)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn uda_extra_success() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create a single UTXO to issue assets on the same UTXO
    party.create_utxos(true, Some(1), None, FEE_RATE, None);

    // issue NIA
    let asset_nia = party.issue_asset_nia(None);

    // issue UDA
    let _asset_uda = party.issue_asset_uda(None, None, vec![]);

    let receive_data = rcv_party.blind_receive();

    // send NIA
    let recipient_map = HashMap::from([(
        asset_nia.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    let (transfers, asset_transfers, _) = party.get_test_transfers_sender(&txid);
    assert_eq!(asset_transfers.len(), 2);
    assert_eq!(transfers.len(), 2);
    let asset_transfer_1 = &asset_transfers[0];
    assert!(asset_transfer_1.user_driven);
    let asset_transfer_2 = &asset_transfers[1];
    assert!(!asset_transfer_2.user_driven);
    let extra_colorings = party.db_colorings_filtered(asset_transfer_2.idx);
    let extra_coloring_input = &extra_colorings[0];
    let extra_coloring_change = &extra_colorings[1];
    assert_eq!(extra_coloring_input.r#type, ColoringType::Input);
    assert_eq!(extra_coloring_input.assignment, Assignment::NonFungible);
    assert_eq!(extra_coloring_change.r#type, ColoringType::Change);
    assert_eq!(extra_coloring_change.assignment, Assignment::NonFungible);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn psbt_rgb_consumer_success() {
    initialize();

    // create wallet with funds and no UTXOs
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create 1 UTXO
    println!("utxo 1");
    party.create_utxos(true, Some(1), None, FEE_RATE, None);
    party.show_unspent_colorings("after create utxos 1");

    // issue a NIA asset
    println!("issue 1");
    let asset_nia_a = party.issue_asset_nia(None);
    party.show_unspent_colorings("after issue 1");

    // issue a 2nd NIA asset on the same UTXO
    println!("issue 2");
    let asset_nia_b = party.issue_asset_nia(None);
    party.show_unspent_colorings("after issue 2");

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // try to send the 1st asset
    println!("send_begin 1");
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map).unwrap();
    assert!(!result.psbt.is_empty());
    party.show_unspent_colorings("after send 1");
    party.fail_transfers_single(result.batch_transfer_idx.unwrap());

    // try to send the 2nd asset
    println!("send_begin 2");
    let receive_data_2 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia_b.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map).unwrap();
    assert!(!result.psbt.is_empty());
    party.show_unspent_colorings("after send 2");
    party.fail_transfers_single(result.batch_transfer_idx.unwrap());

    // try to send the 1st asset to a recipient and the 2nd to different one
    println!("send_begin 3");
    let receive_data_3a = rcv_party.blind_receive();
    let receive_data_3b = rcv_party.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_nia_a.asset_id,
            vec![Recipient {
                assignment: Assignment::Fungible(1),
                recipient_id: receive_data_3a.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_nia_b.asset_id,
            vec![Recipient {
                assignment: Assignment::Fungible(1),
                recipient_id: receive_data_3b.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let result = party.send_begin_result(&recipient_map).unwrap();
    assert!(!result.psbt.is_empty());
    party.show_unspent_colorings("after send 3");
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_bitcoins() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create 1 UTXO with not enough bitcoins for a send and send the rest
    party.create_utxos(false, Some(1), Some(TINY_BTC_AMOUNT), FEE_RATE, None);
    party.send_btc(&rcv_party.get_address(), 99_999_000);

    // issue an NIA asset
    let asset_nia_a = party.issue_asset_nia(None);

    // send with no colorable UTXOs available as additional bitcoin inputs and no other funds
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 1);
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 UTXO for change (add funds, create UTXO, send the rest)
    fund_wallet(party.get_address());
    party.create_utxos(false, Some(1), Some(TINY_BTC_AMOUNT), FEE_RATE, None);
    party.send_btc(&rcv_party.get_address(), 99_999_000);

    // send works with no colorable UTXOs available as additional bitcoin inputs
    party.wait_for_unspents(false, 2);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_allocations_fail() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create 1 UTXO with not enough bitcoins for a send
    party.create_utxos(false, Some(1), Some(TINY_BTC_AMOUNT), FEE_RATE, None);

    // issue an NIA asset
    let asset_nia_a = party.issue_asset_nia(None);

    // send with no colorable UTXOs available as change
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 2);
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    party.create_utxos(false, Some(1), None, FEE_RATE, None);

    // send works with no colorable UTXOs available as additional bitcoin inputs
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 3);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_allocations_success() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // create 1 UTXO with not enough bitcoins for a send
    party.create_utxos(false, Some(1), Some(TINY_BTC_AMOUNT), FEE_RATE, None);

    // issue an NIA asset on the unspendable UTXO
    let asset_nia_a = party.issue_asset_nia(None);

    // create 2 more UTXOs, 1 for change + 1 as additional bitcoin input
    party.create_utxos(false, Some(2), None, FEE_RATE, None);

    // send with 1 colorable UTXOs available as additional bitcoin input
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            assignment: Assignment::Fungible(1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map).unwrap();
    assert!(!result.psbt.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_to_oneself() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data_1 = party.blind_receive();
    let receive_data_2 = party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_1),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount_2),
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

    // transfers progress to status Settled after refreshes
    party.wait_for_refresh_raw(None, Some(&[2, 3]));
    mine(false);
    party.wait_for_refresh(None);

    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    assert_eq!(batch_transfers.len(), 3);
    assert!(
        batch_transfers
            .iter()
            .all(|t| t.status == TransferStatus::Settled)
    );

    // check balance is unchanged
    let nia_assets = party.list_assets(&[AssetSchema::Nia]).nia.unwrap();
    let asset = nia_assets.first().unwrap();
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );

    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 5);
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::Issuance)
            .count(),
        1
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::Send)
            .count(),
        2
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::ReceiveBlind)
            .count(),
        1
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::ReceiveWitness)
            .count(),
        1
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_to_oneself_crash() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallet
    let mut party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // self-send recipients
    let receive_data_1 = party.blind_receive();
    let receive_data_2 = party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_1),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount_2),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);

    // send_begin with dry_run=true (matches the default send() path) to persists no transfer
    let begin = party
        .wallet
        .send_begin(
            party.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            true,
        )
        .unwrap();
    let signed_psbt = party.wallet.sign_psbt(begin.psbt, None).unwrap();

    // first send_end "crashes" after posting the consignment but with no send transfer yet
    println!("setting MOCK_SEND_END_CRASH");
    MOCK_SEND_END_CRASH.replace(Some(()));
    let crashed = party.wallet.send_end(party.online, signed_psbt.clone());
    assert_matches!(crashed, Err(Error::Internal { .. }));

    // chcek the send transfer was not persisted
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::Send)
            .count(),
        0
    );

    // refreshing the incoming transfers accepts the (already posted) consignment and stamps
    // the send txid onto them, so the receive transfers now share the send transaction's txid
    party.wait_for_refresh_raw(None, Some(&[2, 3]));

    // completing the send must still create the send transfers despite the receive transfers
    // already carrying the same txid
    let res = party.wallet.send_end(party.online, signed_psbt).unwrap();
    assert!(!res.txid.is_empty());

    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 5);
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::Issuance)
            .count(),
        1
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::Send)
            .count(),
        2
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::ReceiveBlind)
            .count(),
        1
    );
    assert_eq!(
        transfers
            .iter()
            .filter(|t| t.kind == TransferKind::ReceiveWitness)
            .count(),
        1
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_back_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    //
    // 1st transfer: from issuer to recipient
    //

    // send
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party.send_retry(&recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // transfer 1 checks
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_1);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount_1)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(AMOUNT - amount_1)]
    );

    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(AMOUNT - amount_1)
    );

    //
    // 2nd transfer: from recipient back to issuer
    //

    // send
    let receive_data_2 = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = rcv_party.send_retry(&recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));

    // transfer 2 checks
    let rcv_transfer = party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = rcv_party.get_test_transfer_sender(&txid_2);
    let (transfer_data, _) = rcv_party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount_1 - amount_2)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(amount_2)]
    );

    let unspents = rcv_party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(amount_1 - amount_2)
    );

    //
    // 3rd transfer: from issuer to recipient once again (spend what was received back)
    //

    // send
    party.show_unspent_colorings("wallet before 3rd transfer");
    rcv_party.show_unspent_colorings("rcv_wallet before 3rd transfer");
    let receive_data_3 = rcv_party.blind_receive();
    let change_3 = 5;
    let amount_3 = party.get_asset_balance(&asset.asset_id).spendable - change_3;
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_3),
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = party.send_retry(&recipient_map);
    assert!(!txid_3.is_empty());
    party.show_unspent_colorings("wallet after 3rd transfer");
    rcv_party.show_unspent_colorings("rcv_wallet after 3rd transfer");

    // take transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // transfer 3 checks
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_3);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount_3)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(change_3)]
    );

    let unspents = party.list_unspents(true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().assignment,
        Assignment::Fungible(change_3)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data = rcv_party.witness_receive();
    let receive_data_2 = rcv_party.witness_receive();
    let receive_data_3 = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount * 2),
                recipient_id: receive_data_2.recipient_id,
                witness_data: Some(WitnessData {
                    amount_sat: 1200,
                    blinding: Some(7777),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount * 3),
                recipient_id: receive_data_3.recipient_id,
                witness_data: Some(WitnessData {
                    amount_sat: 1400,
                    blinding: Some(8888),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    party.create_utxos(false, None, None, FEE_RATE, None);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) = rcv_party.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(Some(&asset.asset_id));
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = party.get_test_asset_transfer(batch_transfer.idx);
    let transfers = party.db_transfers_filtered(asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = party.get_test_transfer_data(&transfer);
        assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
        // ack is now true on the sender side
        assert_eq!(transfer.ack, Some(true));
    }

    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveWitness);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount)]
    );
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer.asset_id, Some(asset.asset_id.clone()));

    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let nia_assets = rcv_assets.nia.unwrap();
    let cfa_assets = rcv_assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(cfa_assets.len(), 0);
    let rcv_asset = nia_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(rcv_asset.ticker, TICKER);
    assert_eq!(rcv_asset.name, NAME);
    assert_eq!(rcv_asset.precision, PRECISION);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: amount * 6,
            spendable: 0,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = party.get_test_asset_transfer(batch_transfer.idx);
    let transfers: Vec<DbTransfer> = party.db_transfers_filtered(asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = party.get_test_transfer_data(&transfer);
        assert_eq!(transfer_data.status, TransferStatus::Settled);
        // change is unspent once transfer is Settled
        let unspents = party.list_unspents(true);
        let change_unspent = unspents
            .into_iter()
            .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo);
        assert!(change_unspent.is_some());
    }
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);

    let balances = rcv_party.get_btc_balance_with_sync();
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 8600,
            future: 8600,
            spendable: 8600,
        }
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_multiple_assets_success() {
    initialize();

    let amount: u64 = 66;
    let btc_amount_1a = 1600;
    let btc_amount_1b = 1200;
    let btc_amount_2a = 1000;
    let btc_amount_2b = 1400;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset_1 = party.issue_asset_nia(None);
    let asset_2 = party.issue_asset_nia(None);

    // send 1: check a transfer of multiple assets with multiple recepients works as expected
    println!("\nsend 1");
    let receive_data_1a = rcv_party.witness_receive();
    let receive_data_1b = rcv_party.witness_receive();
    let receive_data_2a = rcv_party.witness_receive();
    let receive_data_2b = rcv_party.witness_receive();
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount),
                    recipient_id: receive_data_1a.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_1a,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount * 2),
                    recipient_id: receive_data_1b.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_1b,
                        blinding: Some(7777),
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            asset_2.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount * 3),
                    recipient_id: receive_data_2a.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_2a,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount * 4),
                    recipient_id: receive_data_2b.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_2b,
                        blinding: Some(8888),
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
    ]);
    party.create_utxos(false, None, None, FEE_RATE, None);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    // check receiver transfers
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    let rcv_xfer_1a = rcv_party.get_test_transfer_recipient(&receive_data_1a.recipient_id);
    let rcv_xfer_1b = rcv_party.get_test_transfer_recipient(&receive_data_1b.recipient_id);
    let rcv_xfer_2a = rcv_party.get_test_transfer_recipient(&receive_data_2a.recipient_id);
    let rcv_xfer_2b = rcv_party.get_test_transfer_recipient(&receive_data_2b.recipient_id);
    let (rcv_xfer_data_1a, rcv_asset_xfer_1a) = rcv_party.get_test_transfer_data(&rcv_xfer_1a);
    let (rcv_xfer_data_1b, rcv_asset_xfer_1b) = rcv_party.get_test_transfer_data(&rcv_xfer_1b);
    let (rcv_xfer_data_2a, rcv_asset_xfer_2a) = rcv_party.get_test_transfer_data(&rcv_xfer_2a);
    let (rcv_xfer_data_2b, rcv_asset_xfer_2b) = rcv_party.get_test_transfer_data(&rcv_xfer_2b);
    assert_eq!(rcv_xfer_data_1a.kind, TransferKind::ReceiveWitness);
    assert_eq!(rcv_xfer_data_1b.kind, TransferKind::ReceiveWitness);
    assert_eq!(rcv_xfer_data_2a.kind, TransferKind::ReceiveWitness);
    assert_eq!(rcv_xfer_data_2b.kind, TransferKind::ReceiveWitness);
    assert_eq!(
        rcv_xfer_data_1a.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_xfer_data_1b.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_xfer_data_2a.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_xfer_data_2b.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        rcv_xfer_data_1a.assignments,
        vec![Assignment::Fungible(amount)]
    );
    assert_eq!(
        rcv_xfer_data_1b.assignments,
        vec![Assignment::Fungible(amount * 2)]
    );
    assert_eq!(
        rcv_xfer_data_2a.assignments,
        vec![Assignment::Fungible(amount * 3)]
    );
    assert_eq!(
        rcv_xfer_data_2b.assignments,
        vec![Assignment::Fungible(amount * 4)]
    );
    assert_eq!(rcv_asset_xfer_1a.asset_id, Some(asset_1.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_1b.asset_id, Some(asset_1.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_2a.asset_id, Some(asset_2.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_2b.asset_id, Some(asset_2.asset_id.clone()));
    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let nia_assets = rcv_assets.nia.unwrap();
    let cfa_assets = rcv_assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 2);
    assert_eq!(cfa_assets.len(), 0);
    let rcv_asset_1 = nia_assets.first().unwrap();
    let rcv_asset_2 = nia_assets.last().unwrap();
    assert_eq!(rcv_asset_1.asset_id, asset_1.asset_id);
    assert_eq!(rcv_asset_2.asset_id, asset_2.asset_id);
    assert_eq!(
        rcv_asset_1.balance,
        Balance {
            settled: 0,
            future: amount * 3,
            spendable: 0,
        }
    );
    assert_eq!(
        rcv_asset_2.balance,
        Balance {
            settled: 0,
            future: amount * 7,
            spendable: 0,
        }
    );
    // transfer vout + BTC amount match tx outputs
    #[allow(unreachable_patterns)]
    let tx_details = match rcv_party.wallet.indexer() {
        Indexer::Electrum(client) => client
            .inner
            .raw_call(
                "blockchain.transaction.get",
                vec![Param::String(txid.clone()), Param::Bool(true)],
            )
            .unwrap(),
        _ => unreachable!("wallet using electrum"),
    };
    let tx_outputs = tx_details.get("vout").unwrap().as_array().unwrap();
    for (rcv_xfer, btc_amt) in [
        (rcv_xfer_1a, btc_amount_1a),
        (rcv_xfer_1b, btc_amount_1b),
        (rcv_xfer_2a, btc_amount_2a),
        (rcv_xfer_2b, btc_amount_2b),
    ] {
        let RecipientTypeFull::Witness { vout } = rcv_xfer.recipient_type.unwrap() else {
            panic!()
        };
        let transfer_vout = vout.unwrap() as u64;
        let tx_out = tx_outputs
            .iter()
            .find(|o| o.get("n").unwrap().as_number().unwrap().as_u64().unwrap() == transfer_vout)
            .unwrap();
        let tx_vout_amount = tx_out
            .get("value")
            .unwrap()
            .as_number()
            .unwrap()
            .as_f64()
            .unwrap()
            * 100_000_000.0;
        assert_eq!(btc_amt, tx_vout_amount as u64);
    }

    // check sender transfers
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = party.db_asset_transfers_filtered(batch_transfer.idx);
    assert_eq!(asset_transfers.len(), 2);
    let transfers_1 = party.db_transfers_filtered(asset_transfers.first().unwrap().idx);
    let transfers_2 = party.db_transfers_filtered(asset_transfers.last().unwrap().idx);
    transfers_1.into_iter().chain(transfers_2).for_each(|t| {
        let (transfer_data, _) = party.get_test_transfer_data(&t);
        assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
    });

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    // check receiver transfers
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    let rcv_xfer_1a = rcv_party.get_test_transfer_recipient(&receive_data_1a.recipient_id);
    let rcv_xfer_1b = rcv_party.get_test_transfer_recipient(&receive_data_1b.recipient_id);
    let rcv_xfer_2a = rcv_party.get_test_transfer_recipient(&receive_data_2a.recipient_id);
    let rcv_xfer_2b = rcv_party.get_test_transfer_recipient(&receive_data_2b.recipient_id);
    let (rcv_xfer_data_1a, _) = rcv_party.get_test_transfer_data(&rcv_xfer_1a);
    let (rcv_xfer_data_1b, _) = rcv_party.get_test_transfer_data(&rcv_xfer_1b);
    let (rcv_xfer_data_2a, _) = rcv_party.get_test_transfer_data(&rcv_xfer_2a);
    let (rcv_xfer_data_2b, _) = rcv_party.get_test_transfer_data(&rcv_xfer_2b);
    assert_eq!(rcv_xfer_data_1a.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_1b.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_2a.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_2b.status, TransferStatus::Settled);
    let rcv_balances = rcv_party.get_btc_balance_with_sync();
    assert!(matches!(
        rcv_balances.colored,
        Balance {
            settled: 10200,
            future: 10200,
            spendable: 10200,
        }
    ));

    // check sender transfers
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = party.db_asset_transfers_filtered(batch_transfer.idx);
    assert_eq!(asset_transfers.len(), 2);
    let transfers_1 = party.db_transfers_filtered(asset_transfers.first().unwrap().idx);
    let transfers_2 = party.db_transfers_filtered(asset_transfers.last().unwrap().idx);
    transfers_1.into_iter().chain(transfers_2).for_each(|t| {
        let (transfer_data, _) = party.get_test_transfer_data(&t);
        assert_eq!(transfer_data.status, TransferStatus::Settled);
    });
    // asset has been received correctly
    let rcv_assets = rcv_party.list_assets(&[]);
    let nia_assets = rcv_assets.nia.unwrap();
    let cfa_assets = rcv_assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 2);
    assert_eq!(cfa_assets.len(), 0);
    let rcv_asset_1 = nia_assets.first().unwrap();
    let rcv_asset_2 = nia_assets.last().unwrap();
    assert_eq!(rcv_asset_1.asset_id, asset_1.asset_id);
    assert_eq!(rcv_asset_2.asset_id, asset_2.asset_id);
    assert_eq!(
        rcv_asset_1.balance,
        Balance {
            settled: amount * 3,
            future: amount * 3,
            spendable: amount * 3,
        }
    );
    assert_eq!(
        rcv_asset_2.balance,
        Balance {
            settled: amount * 7,
            future: amount * 7,
            spendable: amount * 7,
        }
    );

    // send 2: check get_asset_balance works with a pending witness receive with no asset ID
    println!("\nsend 2");
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount * 5),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // check receiver transfer
    let rcv_xfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_xfer_data, rcv_asset_xfer) = rcv_party.get_test_transfer_data(&rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::WaitingCounterparty);
    assert_eq!(rcv_xfer.requested_assignment, Some(Assignment::Any));
    assert_eq!(rcv_asset_xfer.asset_id, None);
    // check asset balance: pending witness transfer not counted (no asset ID)
    let asset_1_balance = rcv_party.get_asset_balance(&asset_1.asset_id);
    assert_eq!(
        asset_1_balance,
        Balance {
            settled: amount * 3,
            future: amount * 3,
            spendable: amount * 3,
        }
    );

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // check receiver transfer
    let rcv_xfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_xfer_data, rcv_asset_xfer) = rcv_party.get_test_transfer_data(&rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::WaitingConfirmations);
    assert_eq!(
        rcv_xfer_data.assignments,
        vec![Assignment::Fungible(amount * 5)]
    );
    assert_eq!(rcv_asset_xfer.asset_id, Some(asset_1.asset_id.clone()));
    // check asset balance: pending witness transfer counted (future)
    let asset_1_balance = rcv_party.get_asset_balance(&asset_1.asset_id);
    assert_eq!(
        asset_1_balance,
        Balance {
            settled: amount * 3,
            future: amount * 8,
            spendable: amount * 3,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // check receiver transfer
    let rcv_xfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_xfer_data, _) = rcv_party.get_test_transfer_data(&rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::Settled);
    // check asset balance: pending witness transfer counted (settled + spendable as well)
    let asset_1_balance = rcv_party.get_asset_balance(&asset_1.asset_id);
    assert_eq!(
        asset_1_balance,
        Balance {
            settled: amount * 8,
            future: amount * 8,
            spendable: amount * 8,
        }
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_multiple_inputs_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_nia(None);

    // send
    println!("send 1");
    let receive_data_1a = party_2.witness_receive();
    let receive_data_1b = party_2.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_1a.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount * 2),
                recipient_id: receive_data_1b.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1200,
                    blinding: Some(7777),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));

    println!("send 2");
    let receive_data_2 = party_1.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(77),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));

    println!("send 3");
    let receive_data_3 = party_2.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(40),
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    // check transfers have settled
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer = party_2.get_test_transfer_recipient(&receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = party_2.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_1.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party_1.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_fail_wrong_vout() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party_1 = get_funded_party!();
    let mut rcv_party_2 = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data_1 = rcv_party_1.witness_receive();
    let receive_data_2 = rcv_party_2.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount * 2),
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 2000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    println!("setting MOCK_VOUT");
    MOCK_VOUT.replace(Some(2));
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status Failed after a refresh
    rcv_party_2.wait_for_refresh(None);
    rcv_party_1.wait_for_refresh(None);
    let rcv_transfer = rcv_party_1.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (rcv_transfer_data, _rcv_asset_transfer) =
        rcv_party_1.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(Some(&asset.asset_id));
    let batch_transfers = party.db_batch_transfers_filtered(&txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = party.get_test_asset_transfer(batch_transfer.idx);
    let transfers = party.db_transfers_filtered(asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = party.get_test_transfer_data(&transfer);
        assert_eq!(transfer_data.status, TransferStatus::Failed);
    }
    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveWitness);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Failed);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn min_confirmations_common(
    party: &mut SinglesigParty,
    rcv_party: &mut SinglesigParty,
    esplora: bool,
) {
    let amount: u64 = 66;

    // 2 minimum confirmations
    println!("2 confirmations");
    let min_confirmations = 2;

    // issue
    let asset = party.issue_asset_nia(None);

    // avoid bitcoind sync issues
    let _guard = stop_mining_when_alone();
    force_mine_no_resume_when_alone(esplora);

    // send
    let receive_data = rcv_party
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party
        .wallet
        .send(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            min_confirmations,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (_, rcv_batch_transfer) = rcv_party.get_test_transfer_related(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    let (_, batch_transfer) = party.get_test_transfer_related(&transfer);
    assert_eq!(rcv_batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers remain in status WaitingConfirmations after a block is mined
    force_mine_no_resume_when_alone(esplora);
    assert!(!rcv_party.refresh_all());
    assert!(!party.refresh_asset(&asset.asset_id));

    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after a second block is mined
    force_mine_no_resume_when_alone(esplora);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // 0 minimum confirmations
    println!("0 confirmations");
    let min_confirmations = 0;

    // send
    let receive_data = rcv_party
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party
        .wallet
        .send(
            party.online,
            recipient_map,
            false,
            FEE_RATE,
            min_confirmations,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (_, rcv_batch_transfer) = rcv_party.get_test_transfer_related(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    let (_, batch_transfer) = party.get_test_transfer_related(&transfer);
    assert_eq!(rcv_batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled before a block is mined
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // spend received allocations
    let receive_data = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount * 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = rcv_party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    force_mine_no_resume_when_alone(esplora);
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));
    drop(_guard);
    mine(esplora);
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(Some(&asset.asset_id));

    let rcv_transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = rcv_party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = rcv_party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn min_confirmations_electrum() {
    initialize();

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    min_confirmations_common(&mut party, &mut rcv_party, false);
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn min_confirmations_esplora() {
    initialize();

    // wallets
    let mut party = get_funded_party!(ESPLORA_URL.to_string());
    let mut rcv_party = get_funded_party!(ESPLORA_URL.to_string());

    min_confirmations_common(&mut party, &mut rcv_party, true);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn spend_double_receive() {
    initialize();

    let amount_1 = 100;
    let amount_2 = 200;

    // wallets
    // create bigger UTXOs for wallet_2 so a single one can support a witness transfer
    let mut party_1 = get_funded_party!();
    let mut party_3 = get_funded_party!();
    let mut party_2 = get_funded_noutxo_party!();
    party_2.create_utxos(false, None, Some(5000), FEE_RATE, None);

    // issue
    println!("issue");
    let asset = party_1.issue_asset_nia(None);

    // send a first time 1->2 (blind)
    println!("send blind 1->2");
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1
        .wallet
        .send(
            party_1.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid_1.is_empty());
    // settle transfer
    mine(false);
    // check transfer status
    party_2.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = party_2.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_2.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_1.get_test_transfer_sender(&txid_1);
    let (transfer_data, _) = party_1.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // send a second time 1->2 (witness, so the 2 allocations can't be on the same UTXO)
    println!("send witness 1->2");
    let receive_data = party_2.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: Some(777),
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_1
        .wallet
        .send(
            party_1.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid_2.is_empty());
    // settle transfer
    mine(false);
    // check transfer status
    party_2.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = party_2.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_2.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_1.get_test_transfer_sender(&txid_2);
    let (transfer_data, _) = party_1.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check wallet_2 has the 2 expected allocations
    let unspents = party_2.list_unspents(true);
    let asset_unspents: Vec<&Unspent> = unspents
        .iter()
        .filter(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset.asset_id.clone()))
        })
        .collect();
    assert_eq!(asset_unspents.len(), 2);
    assert!(
        asset_unspents
            .first()
            .unwrap()
            .rgb_allocations
            .iter()
            .any(|a| if let Assignment::Fungible(amt) = a.assignment {
                amt == amount_1
            } else {
                false
            })
    );
    assert!(
        asset_unspents
            .last()
            .unwrap()
            .rgb_allocations
            .iter()
            .any(|a| if let Assignment::Fungible(amt) = a.assignment {
                amt == amount_2
            } else {
                false
            })
    );

    // send 2->3, manually selecting the 1st allocation (blind, amount_1) only
    println!("send witness 2->3");
    let receive_data = party_3.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // manually set the input unspents to the UTXO of the 1st allocation
    let db_data = party_2.db_data(false);
    let utxos = party_2.db_unspent_txos(db_data.txos.clone());
    let mut input_unspents = party_2.db_rgb_allocations(
        utxos,
        Some(db_data.colorings.clone()),
        Some(db_data.batch_transfers.clone()),
        Some(db_data.asset_transfers.clone()),
        Some(db_data.transfers.clone()),
    );
    input_unspents.retain(|u| {
        !u.rgb_allocations.is_empty()
            && u.rgb_allocations.iter().all(|a| {
                if let Assignment::Fungible(amt) = a.assignment {
                    amt == amount_1
                } else {
                    false
                }
            })
    });
    assert_eq!(input_unspents.len(), 1);
    println!("setting MOCK_INPUT_UNSPENTS");
    MOCK_INPUT_UNSPENTS.with_borrow_mut(|v| v.push(input_unspents.first().unwrap().clone()));
    // send (will use the manually-selected input unspent)
    let txid_3 = party_2.send_retry(&recipient_map);
    assert!(!txid_3.is_empty());
    // settle transfer
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    // check transfer status
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(Some(&asset.asset_id));
    let rcv_transfer = party_3.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = party_3.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party_2.get_test_transfer_sender(&txid_3);
    let (transfer_data, _) = party_2.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check final balances
    let balance_1 = party_1.get_asset_balance(&asset.asset_id);
    let balance_2 = party_2.get_asset_balance(&asset.asset_id);
    let balance_3 = party_3.get_asset_balance(&asset.asset_id);
    assert_eq!(
        balance_1,
        Balance {
            settled: AMOUNT - amount_1 - amount_2,
            future: AMOUNT - amount_1 - amount_2,
            spendable: AMOUNT - amount_1 - amount_2,
        }
    );
    assert_eq!(
        balance_2,
        Balance {
            settled: amount_2,
            future: amount_2,
            spendable: amount_2,
        }
    );
    assert_eq!(
        balance_3,
        Balance {
            settled: amount_1,
            future: amount_1,
            spendable: amount_1,
        }
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn input_sorting() {
    initialize();

    let amounts: Vec<u64> = vec![444, 222, 555, 111, 333];
    let amount: u64 = 120;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue (allocations not sorted)
    let asset = party.issue_asset_nia(Some(&amounts));

    // send, spending the 111 and 222 allocations
    println!("\nsend 1");
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle transfers
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // check the intended UTXOs have been used
    let unspents = party.list_unspents(false);
    let allocations: Vec<&RgbAllocation> =
        unspents.iter().flat_map(|e| &e.rgb_allocations).collect();
    let mut cur_amounts: Vec<u64> = allocations
        .iter()
        .map(|a| {
            if let Assignment::Fungible(amt) = a.assignment {
                amt
            } else {
                0
            }
        })
        .collect();
    cur_amounts.sort();
    let mut expected_amounts = amounts.clone();
    expected_amounts.retain(|a| *a != 111 && *a != 222);
    expected_amounts.push(111 + 222 - amount);
    expected_amounts.sort();
    assert_eq!(cur_amounts, expected_amounts);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn spend_witness_receive_utxo() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_noutxo_party!();

    // issue
    let asset_a = party_1.issue_asset_nia(None);

    // send
    let receive_data_1 = party_2.witness_receive();
    let recipient_map_1 = HashMap::from([(
        asset_a.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map_1);
    assert!(!txid_1.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    party_2.wait_for_refresh(None);
    let transfer_1_recv = party_2.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (transfer_1_recv_data, _) = party_2.get_test_transfer_data(&transfer_1_recv);
    party_1.wait_for_refresh(Some(&asset_a.asset_id));
    let (transfer_1_send, _, _) = party_1.get_test_transfer_sender(&txid_1);
    let (transfer_1_send_data, _) = party_1.get_test_transfer_data(&transfer_1_send);
    assert_eq!(
        transfer_1_recv_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        transfer_1_send_data.status,
        TransferStatus::WaitingConfirmations
    );

    // mine and refresh the sender wallet only (receiver transfer still WaitingConfirmations)
    mine(false);
    party_1.wait_for_refresh(Some(&asset_a.asset_id));

    // sync DB TXOs for the receiver wallet
    party_2
        .create_utxos_begin_result(false, None, None, FEE_RATE)
        .unwrap();

    // make sure the witness receive UTXO is available
    assert!(party_2.list_unspents_with_sync(false).len() > 1);

    // try to issue an asset on the pending witness receive UTXO > should fail
    let result = party_2.issue_asset_nia_result(Some(&[AMOUNT]));
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn rgb_change_on_btc_change() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let asset = party.issue_asset_nia(None);

    // send with no available colorable UTXOs (need to allocate change to BTC change UTXO)
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // RGB change has been allocated to the same UTXO as the BTC change (exists = false)
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    let unspents = party.list_unspents(false);
    assert_eq!(
        unspents
            .iter()
            .filter(|u| u.utxo.colorable)
            .collect::<Vec<&Unspent>>()
            .len(),
        2
    );
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    assert!(!change_unspent.utxo.exists);
    let change_rgb_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_rgb_allocations.len(), 1);
    let allocation = change_rgb_allocations.first().unwrap();
    assert_eq!(allocation.asset_id, Some(asset.asset_id));
    assert_eq!(allocation.assignment, Assignment::Fungible(AMOUNT - amount));

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_party.wait_for_refresh(None);
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    party.wait_for_refresh(None);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);

    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after tx mining + refresh
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_inexistent_utxos() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_empty_party!();

    // create 1 UTXO
    let size = Some(UTXO_SIZE * 2);
    party.create_utxos(true, Some(1), size, FEE_RATE, None);

    // issue
    let asset = party.issue_asset_nia(Some(&[AMOUNT]));

    // send
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: UTXO_SIZE as u64,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    party.show_unspent_colorings("after send (WaitingCounterparty)");

    // 1 UTXO being spent, 1 UTXO with exists = false
    // trying to get an UTXO for a blind receive should fail
    let result = party.blind_receive_result();
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    // trying to issue an asset should fail
    let result = party.issue_asset_nia_result(None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    let result = party.issue_asset_cfa_result(None, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    let result = party.issue_asset_uda_result(None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    // trying to create 1 UTXO with up_to = true should create 1
    party.create_utxos(true, Some(1), None, FEE_RATE, None);

    // 1 UTXO being spent, 1 UTXO with exists = false, 1 new UTXO
    // issuing an asset should now succeed
    let asset_2 = party.issue_asset_nia(Some(&[AMOUNT * 2]));

    party.show_unspent_colorings("after 2nd issue");

    // 1 UTXO being spent, 1 UTXO with exists = false, 1 UTXO with an allocated asset
    // trying to send more BTC than what's available in the UTXO being spent should fail
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: UTXO_SIZE as u64,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party.send_begin_result(&recipient_map);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn min_fee_rate() {
    initialize();

    let amount: u64 = 66;
    let amount_sat: u64 = 698;
    let fee_rate = 1;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // prepare transfer data
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        (1..=5)
            .map(|_| {
                let receive_data = rcv_party.witness_receive();
                Recipient {
                    assignment: Assignment::Fungible(amount),
                    recipient_id: receive_data.recipient_id,
                    witness_data: Some(WitnessData {
                        amount_sat,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                }
            })
            .collect(),
    )]);

    // check fee amount is the expected one
    let res = party
        .wallet
        .send_begin(
            party.online,
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let psbt = Psbt::from_str(&res.psbt).unwrap();
    let fee = psbt.fee().unwrap().to_sat();
    assert_eq!(fee, 510);

    // actual send
    party.fail_transfers_single(res.batch_transfer_idx.unwrap());
    let txid = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    // ACK transfer
    // broadcast tx
    rcv_party.wait_for_refresh(None);
    assert!(party.refresh_asset(&asset.asset_id));
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn max_fee_exceeded_common(
    asset_id: &str,
    party: &mut SinglesigParty,
    rcv_party: &mut SinglesigParty,
    transfer_idx: i32,
) {
    let fee_rate = 20000;
    let amount = AMOUNT_SMALL;
    let amount_sat: u64 = 698;

    // get a lot of funds
    (0..9).for_each(|_| fund_wallet(party.get_address()));
    party.create_utxos(false, Some(20), Some(u32::MAX / 100), FEE_RATE, None);

    // prepare transfer data
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset_id.to_string(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);

    // send
    let send_result = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();
    assert!(!send_result.txid.is_empty());

    // ACK transfer
    // broadcast tx
    rcv_party.wait_for_refresh(None);
    let result = party.refresh_result(None, &[]).unwrap();
    assert_eq!(
        result,
        HashMap::from([(
            transfer_idx,
            RefreshedTransfer {
                updated_status: None,
                failure: Some(Error::MaxFeeExceeded {
                    txid: send_result.txid.clone()
                }),
            }
        )])
    );
    party.fail_transfers_single(send_result.batch_transfer_idx);
    let result = party.refresh_result(None, &[]).unwrap();
    assert_eq!(result, HashMap::new());
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn max_fee_exceeded_electrum() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    let asset = party.issue_asset_nia(None);

    max_fee_exceeded_common(&asset.asset_id, &mut party, &mut rcv_party, 2);
}

#[cfg(feature = "esplora")]
#[test]
#[serial]
fn max_fee_exceeded_esplora() {
    initialize();

    let mut party = get_funded_party!(ESPLORA_URL.to_string());
    let mut rcv_party = get_empty_party!(ESPLORA_URL.to_string());

    let asset = party.issue_asset_nia(None);

    max_fee_exceeded_common(&asset.asset_id, &mut party, &mut rcv_party, 2);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn min_relay_fee_common(
    asset_id: &str,
    party: &mut SinglesigParty,
    rcv_party: &mut SinglesigParty,
    transfer_idx: i32,
) {
    let fee_rate = 0;
    let amount = AMOUNT_SMALL;
    let amount_sat: u64 = 698;

    // prepare transfer data
    let recipient_map = HashMap::from([(
        asset_id.to_string(),
        (1..=5)
            .map(|_| {
                let receive_data = rcv_party.witness_receive();
                Recipient {
                    assignment: Assignment::Fungible(amount),
                    recipient_id: receive_data.recipient_id,
                    witness_data: Some(WitnessData {
                        amount_sat,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                }
            })
            .collect(),
    )]);

    // check fee amount is the expected one
    println!("setting MOCK_CHECK_FEE_RATE");
    MOCK_CHECK_FEE_RATE.replace(vec![true, true]);
    let res = party
        .wallet
        .send_begin(
            party.online,
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            None,
            false,
        )
        .unwrap();
    let psbt = Psbt::from_str(&res.psbt).unwrap();
    let fee = psbt.fee().unwrap().to_sat();
    assert_eq!(fee, 0);
    party.fail_transfers_single(res.batch_transfer_idx.unwrap());

    // actual send
    println!("setting MOCK_CHECK_FEE_RATE");
    MOCK_CHECK_FEE_RATE.replace(vec![true, true]);
    let send_result = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();
    assert!(!send_result.txid.is_empty());

    // ACK transfer
    // broadcast tx
    rcv_party.wait_for_refresh(None);
    let result = party.refresh_result(None, &[]).unwrap();
    assert_eq!(
        result,
        HashMap::from([(
            transfer_idx,
            RefreshedTransfer {
                updated_status: None,
                failure: Some(Error::MinFeeNotMet {
                    txid: send_result.txid.clone()
                }),
            }
        )])
    );
    party.fail_transfers_single(send_result.batch_transfer_idx);
    let result = party.refresh_result(None, &[]).unwrap();
    assert_eq!(result, HashMap::new());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn min_relay_fee_electrum() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    let asset = party.issue_asset_nia(None);

    min_relay_fee_common(&asset.asset_id, &mut party, &mut rcv_party, 3);
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn min_relay_fee_esplora() {
    initialize();

    let mut party = get_funded_party!(ESPLORA_URL.to_string());
    let mut rcv_party = get_empty_party!(ESPLORA_URL.to_string());

    let asset = party.issue_asset_nia(None);

    min_relay_fee_common(&asset.asset_id, &mut party, &mut rcv_party, 3);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn script_buf_to_from_recipient_id() {
    initialize();

    // wallets
    let mut party = get_funded_party!();

    // get a script bug from an address (witness receive)
    let address_str = party.get_address();
    let script_buf = party.wallet.get_script_pubkey(&address_str).unwrap();

    // recipient ID from script buf
    let recipient_id = recipient_id_from_script_buf(script_buf.clone(), BitcoinNetwork::Regtest);

    // script buf from recipient ID Some
    let script_from_recipient = script_buf_from_recipient_id(recipient_id).unwrap();

    // checks
    assert!(script_from_recipient.is_some());
    assert_eq!(script_from_recipient.unwrap(), script_buf);

    // script buf from recipient ID None (blinded)
    let receive_data = party.blind_receive();
    let script_from_recipient = script_buf_from_recipient_id(receive_data.recipient_id).unwrap();
    assert!(script_from_recipient.is_none());

    // script buf from recipient ID None (bad recipient ID)
    let result = script_buf_from_recipient_id(s!(""));
    assert!(matches!(result, Err(Error::InvalidRecipientID)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue (2 allocations, 1 per send)
    let issue_amounts = [100, 200];
    let asset = party.issue_asset_nia(Some(&issue_amounts));
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);

    // send (blinded) skipping sync
    let receive_data_1 = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid_1.is_empty());

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_1);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);

    // amount
    let change = issue_amounts[0] - amount;
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(change)]
    );
    assert_eq!(rcv_transfer.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );
    // status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // send (witness) skipping sync
    let receive_data_2 = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid_2.is_empty());

    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_2);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);

    // amount
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(issue_amounts[1] - amount)]
    );
    assert_eq!(rcv_transfer.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );
    // status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // settle transfers
    rcv_party.wait_for_refresh_raw(None, Some(&[1, 2]));
    party.wait_for_refresh_raw(None, Some(&[2, 3]));
    mine(false);
    rcv_party.wait_for_refresh_raw(None, Some(&[1, 2]));
    party.wait_for_refresh_raw(None, Some(&[2, 3]));
    assert!(party.check_test_transfer_status_sender(&txid_1, TransferStatus::Settled));
    assert!(party.check_test_transfer_status_sender(&txid_2, TransferStatus::Settled));

    // send to oneself (witness) skipping sync
    let receive_data_3 = party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid_3.is_empty());

    // transfers are in WaitingCounterparty
    let rcv_transfer = party.get_test_transfer_recipient(&receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = party.get_test_transfer_data(&rcv_transfer);
    let (transfer, _, _) = party.get_test_transfer_sender(&txid_3);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(rcv_transfer.requested_assignment, Some(Assignment::Any));
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // refresh skipping sync
    party
        .wallet
        .refresh(party.online, None, vec![], true)
        .unwrap();

    // transfers are now in WaitingConfirmations
    let rcv_transfer = party.get_test_transfer_recipient(&receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = party.get_test_transfer_data(&rcv_transfer);
    let batch_transfers = party.db_batch_transfers_filtered(&txid_3);
    let batch_transfer = batch_transfers.iter().find(|t| t.idx == 5).unwrap();
    let asset_transfer = party.get_test_asset_transfer(batch_transfer.idx);
    let transfers: Vec<DbTransfer> = party.db_transfers_filtered(asset_transfer.idx);
    assert_eq!(transfers.len(), 1);
    let transfer = transfers.first().unwrap();
    let (transfer_data, _) = party.get_test_transfer_data(transfer);
    assert_eq!(
        rcv_transfer_data.assignments,
        vec![Assignment::Fungible(amount)]
    );
    assert_eq!(
        transfer_data.assignments,
        vec![Assignment::Fungible(asset.issued_supply - amount * 3)]
    );
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations,
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // mine and refresh skipping sync > cannot refresh ReceiveWitness transfer as a sync is needed
    mine(false);
    party
        .wallet
        .refresh(party.online, None, vec![], true)
        .unwrap();
    party.show_unspent_colorings("after refresh 2");

    // Send transfer is now settled
    let batch_transfers = party.db_batch_transfers_filtered(&txid_3);
    let batch_transfer = batch_transfers.iter().find(|t| t.idx == 5).unwrap();
    let asset_transfer = party.get_test_asset_transfer(batch_transfer.idx);
    let transfers: Vec<DbTransfer> = party.db_transfers_filtered(asset_transfer.idx);
    let transfer = transfers.first().unwrap();
    let (transfer_data, _) = party.get_test_transfer_data(transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // sync and refresh again (still skipping sync) > ReceiveWitness transfer now refreshes + new UTXO appears
    party
        .wallet
        .sync(
            party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    party
        .wallet
        .refresh(party.online, None, vec![], true)
        .unwrap();
    party.show_unspent_colorings("after refresh 3");

    // ReceiveWitness transfer is now settled as well
    let rcv_transfer = party.get_test_transfer_recipient(&receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ifa_success() {
    initialize();

    let amount_fungible: u64 = 66;
    let amount_inflation: u64 = 42;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_ifa(None, None, None);
    party.show_unspent_colorings("after issuance");
    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert!(transfers.iter().any(|t| t.kind == TransferKind::Issuance));

    // issuance checks
    let unspents = party.list_unspents(false);
    let mut allocations = unspents.iter().flat_map(|u| &u.rgb_allocations);
    assert!(allocations.any(|a| a.assignment == Assignment::Fungible(AMOUNT)));
    assert!(allocations.any(|a| a.assignment == Assignment::InflationRight(AMOUNT_INFLATION)));

    // send
    let receive_data_fungible = rcv_party.blind_receive();
    let receive_data_inflation = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_fungible),
                recipient_id: receive_data_fungible.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::InflationRight(amount_inflation),
                recipient_id: receive_data_inflation.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    party.show_unspent_colorings("after send");

    // transfers progress to status Settled after refreshing
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);

    // transfer checks
    let recv_fungible = rcv_party.get_test_transfer_recipient(&receive_data_fungible.recipient_id);
    let (recv_fungible_data, _) = rcv_party.get_test_transfer_data(&recv_fungible);
    assert_eq!(recv_fungible_data.status, TransferStatus::Settled);
    let recv_inflation =
        rcv_party.get_test_transfer_recipient(&receive_data_inflation.recipient_id);
    let (recv_inflation_data, _) = rcv_party.get_test_transfer_data(&recv_inflation);
    assert_eq!(recv_inflation_data.status, TransferStatus::Settled);

    let transfers = party.list_transfers(Some(&asset.asset_id));
    assert_eq!(transfers.len(), 3);
    let mut sends = transfers.iter().filter(|t| t.kind == TransferKind::Send);
    assert_eq!(sends.clone().count(), 2);
    let send_fungible = sends
        .find(|t| t.requested_assignment == Some(Assignment::Fungible(amount_fungible)))
        .unwrap();
    let send_inflation = sends
        .find(|t| t.requested_assignment == Some(Assignment::InflationRight(amount_inflation)))
        .unwrap();
    assert_eq!(send_fungible.status, TransferStatus::Settled);
    assert_eq!(send_inflation.status, TransferStatus::Settled);

    // change checks
    assert!(send_fungible.change_utxo.is_some());
    assert!(send_inflation.change_utxo.is_some());

    // send all assets to another UTXO
    party.show_unspent_colorings("before asset move");
    let Balance {
        settled: _,
        future: _,
        spendable: asset_total,
    } = party.get_asset_balance(&asset.asset_id);
    let receive_data = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(asset_total),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle the transfers
    party.show_unspent_colorings("after asset send to oneself");
    party.wait_for_refresh(None);
    party.show_unspent_colorings("after asset send to oneself + refresh 1");
    mine(false);
    party.wait_for_refresh(None);

    // send InflationRight only
    // settle the transfers
    party.show_unspent_colorings("before InflationRights move");
    let unspents = party.list_unspents_with_sync(true);
    let inflation_right_amount = unspents
        .iter()
        .flat_map(|u| u.rgb_allocations.clone())
        .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
        .map(|a| a.assignment.inflation_amount())
        .sum::<u64>();
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::InflationRight(inflation_right_amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // check asset allocations are still spendable (not selected as input)
    party.show_unspent_colorings("after InflationRights move");
    let balance = party.get_asset_balance(&asset.asset_id);
    let expected_balance = Balance {
        settled: asset_total,
        future: asset_total,
        spendable: asset_total,
    };
    assert_eq!(balance, expected_balance);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    mine(false);
    // check final balances
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(None);
    party.show_unspent_colorings("after InflationRights move + refresh");
    let balance = party.get_asset_balance(&asset.asset_id);
    let expected_balance = Balance {
        settled: asset_total,
        future: asset_total,
        spendable: asset_total,
    };
    assert_eq!(balance, expected_balance);
    let unspents = party.list_unspents_with_sync(true);
    let inflation_right_amount = unspents
        .iter()
        .flat_map(|u| u.rgb_allocations.clone())
        .filter(|a| matches!(a.assignment, Assignment::InflationRight(_)))
        .map(|a| a.assignment.inflation_amount())
        .sum::<u64>();
    assert_eq!(inflation_right_amount, 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ifa_reject_list() {
    initialize();

    println!("\n=== SCENARIO 1: spending opout in rejectlist - send FAILS then WORKS");
    test_reject_list_scenario_1();

    println!(
        "\n=== SCENARIO 2: spending opout with ancestor in rejectlist - send FAILS then WORKS"
    );
    test_reject_list_scenario_2();

    println!("\n=== SCENARIO 3: spending opout in rejectlist but also in allowlist - WORKS");
    test_reject_list_scenario_3();

    println!(
        "\n=== SCENARIO 4: old ancestor in rejectlist but recent ancestor in allowlist - WORKS"
    );
    test_reject_list_scenario_4();

    println!("\n=== SCENARIO 5: rejected opout in DAG but not in ancestry chain - WORKS");
    test_reject_list_scenario_5();
}

#[cfg(feature = "electrum")]
fn test_reject_list_scenario_1() {
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    let list_name = "reject1.list";

    write_opouts_to_reject_list(list_name, &[]);
    let asset = party_1.issue_asset_ifa(
        Some(&[100, 100]),
        Some(&[50]),
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );

    let receive_data = party_2.blind_receive();
    let amt = 60;
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map_1);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // add to the reject list the newly received allocation
    let opouts = party_2.extract_opouts_from_transfer(&asset.asset_id, &txid);
    assert_eq!(opouts.len(), 1);
    write_opouts_to_reject_list(list_name, &[opouts[0].to_string()]);

    // fail to send from the rejected allocation
    let receive_data = party_3.blind_receive();
    let mut recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(50),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party_2.send_result(&recipient_map_2);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );

    // skip build dag check to see that receiver would refuse
    println!("setting MOCK_SKIP_BUILD_DAG");
    MOCK_SKIP_BUILD_DAG.replace(Some(()));
    let _txid = party_2.send_retry(&recipient_map_2);
    party_3.refresh_all();
    assert!(
        party_3.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );

    // send more assets
    let receive_data = party_2.blind_receive();
    let recipient_map_3 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt + 1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_1.send_retry(&recipient_map_3);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // now the previously failed send works since enough allowed allocations
    let receive_data = party_3.blind_receive(); // avoid RecipientIDAlreadyUsed
    recipient_map_2
        .entry(asset.asset_id)
        .and_modify(|r| r[0].recipient_id = receive_data.recipient_id.clone());
    let _txid = party_2.send_retry(&recipient_map_2);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
}

#[cfg(feature = "electrum")]
fn test_reject_list_scenario_2() {
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();
    let mut party_4 = get_funded_party!();

    let list_name = "reject2.list";

    write_opouts_to_reject_list(list_name, &[]);
    let asset = party_1.issue_asset_ifa(
        Some(&[200]),
        Some(&[50]),
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );

    let receive_data = party_2.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(70),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map_1);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let amt = 60;
    let recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_2.send_retry(&recipient_map_2);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    // add to the reject list the opout from the first transfer (ancestor of the 2nd)
    let opouts = party_2.extract_opouts_from_transfer(&asset.asset_id, &txid);
    assert_eq!(opouts.len(), 1);
    write_opouts_to_reject_list(list_name, &[opouts[0].to_string()]);

    // fail to send from the allocation with a rejected ancestor
    let receive_data = party_4.blind_receive();
    let mut recipient_map_3 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(50),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = party_3.send_result(&recipient_map_3);
    assert_matches!(
        result,
        Err(Error::InsufficientAssignments { asset_id: ref t, .. }) if t == &asset.asset_id
    );

    // skip build dag check to see that receiver would refuse
    println!("setting MOCK_SKIP_BUILD_DAG");
    MOCK_SKIP_BUILD_DAG.replace(Some(()));
    let _txid = party_3.send_retry(&recipient_map_3);
    party_4.refresh_all();
    assert!(
        party_4.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );

    // send more assets
    let receive_data = party_3.blind_receive();
    let recipient_map_4 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt + 1),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_1.send_retry(&recipient_map_4);
    party_3.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // now the previously failed send works since enough allowed allocations
    let receive_data = party_4.blind_receive(); // avoid RecipientIDAlreadyUsed
    recipient_map_3
        .entry(asset.asset_id)
        .and_modify(|r| r[0].recipient_id = receive_data.recipient_id.clone());
    let _txid = party_3.send_retry(&recipient_map_3);
    party_4.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
    mine(false);
    party_4.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
}

#[cfg(feature = "electrum")]
fn test_reject_list_scenario_3() {
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();

    let list_name = "reject3.list";

    write_opouts_to_reject_list(list_name, &[]);
    let asset = party_1.issue_asset_ifa(
        Some(&[100]),
        Some(&[50]),
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );

    let receive_data = party_2.blind_receive();
    let amt = 60;
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amt),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map_1);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // add to the reject list the newly received allocation
    // both in rejected and allowed mode
    let opouts = party_2.extract_opouts_from_transfer(&asset.asset_id, &txid);
    assert_eq!(opouts.len(), 1);
    let opout_str = opouts[0].to_string();
    write_opouts_to_reject_list(list_name, &[opout_str.clone(), format!("!{opout_str}")]);

    // send the newly received allocation (succeeds because the opout has been allowed)
    let receive_data = party_3.blind_receive();
    let recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(50),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_2.send_retry(&recipient_map_2);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
}

#[cfg(feature = "electrum")]
fn test_reject_list_scenario_4() {
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();
    let mut party_4 = get_funded_party!();
    let mut party_5 = get_funded_party!();

    let list_name = "reject4.list";

    write_opouts_to_reject_list(list_name, &[]);
    let asset = party_1.issue_asset_ifa(
        Some(&[100]),
        Some(&[50]),
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );

    let receive_data = party_2.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(80),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_1.send_retry(&recipient_map_1);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(70),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_2.send_retry(&recipient_map_2);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    let receive_data = party_4.blind_receive();
    let recipient_map_3 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(65),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_3.send_retry(&recipient_map_3);
    party_4.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
    mine(false);
    party_4.wait_for_refresh(None);
    party_3.wait_for_refresh(None);

    // add to the reject list the opout from first the first transfer and
    // allow the one from the second transfer (child of the first one)
    let opouts_1 = party_2.extract_opouts_from_transfer(&asset.asset_id, &txid_1);
    assert_eq!(opouts_1.len(), 1);
    let opouts_2 = party_3.extract_opouts_from_transfer(&asset.asset_id, &txid_2);
    assert_eq!(opouts_2.len(), 1);
    write_opouts_to_reject_list(
        list_name,
        &[opouts_1[0].to_string(), format!("!{}", opouts_2[0])],
    );

    // send the newly received allocation (succeeds because the more recent ancestor is allowed)
    let receive_data = party_5.blind_receive();
    let recipient_map_3 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(50),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_4.send_retry(&recipient_map_3);
    party_5.wait_for_refresh(None);
    party_4.wait_for_refresh(None);
    mine(false);
    party_4.wait_for_refresh(None);
    party_5.wait_for_refresh(None);
}

#[cfg(feature = "electrum")]
fn test_reject_list_scenario_5() {
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();
    let mut party_3 = get_funded_party!();
    let mut party_4 = get_funded_party!();

    let list_name = "reject5.list";

    write_opouts_to_reject_list(list_name, &[]);
    let asset = party_1.issue_asset_ifa(
        Some(&[150]),
        Some(&[100]),
        Some(format!("http://localhost:8140/lists/{list_name}")),
    );

    let receive_data = party_2.blind_receive();
    let recipient_map_1 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(60),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map_1);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    let receive_data = party_3.blind_receive();
    let recipient_map_2 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(70),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_1.send_retry(&recipient_map_2);
    party_3.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // add to the reject list the allocation of the first transfer (sibling of the one we are about
    // to spend)
    let opouts = party_2.extract_opouts_from_transfer(&asset.asset_id, &txid);
    assert_eq!(opouts.len(), 1);
    write_opouts_to_reject_list(list_name, &[opouts[0].to_string()]);

    // send the newly received allocation (succeeds because the rejected allocation is in the DAG
    // but not in the opout ancestry chain)
    let receive_data = party_4.blind_receive();
    let recipient_map_3 = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(40),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let _txid = party_3.send_retry(&recipient_map_3);
    party_4.wait_for_refresh(None);
    party_3.wait_for_refresh(None);
    mine(false);
    party_3.wait_for_refresh(None);
    party_4.wait_for_refresh(None);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_witness_ma1_blind_receive_fail() {
    initialize();

    let amount: u64 = 66;

    // sender wallet
    // recipient wallet
    let mut party = get_funded_party!();
    let mut rcv_wallet = get_test_wallet(true, Some(1)); // MAX_ALLOCATIONS_PER_UTXO = 1
    let rcv_online = rcv_wallet.go_online(test_go_online_options(None)).unwrap();
    let mut rcv_party = party!(rcv_wallet, rcv_online);

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let OperationResult { txid, .. } = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            true, // donation, so TX gets broadcast right away
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();
    assert!(!txid.is_empty());

    // sync recipient wallet (no refresh) to see the new UTXO but not the new allocation
    rcv_party
        .wallet
        .sync(
            rcv_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();

    // make sure the recipient wallet sees 1 colorable UTXO with no RGB allocations
    let unspents = rcv_party.list_unspents(false);
    assert_eq!(unspents.len(), 1);
    assert!(
        unspents
            .iter()
            .all(|u| u.utxo.colorable && u.rgb_allocations.is_empty())
    );

    // try to blind the new UTXO: it should error as it already has the max allocation number
    let result = rcv_party.blind_receive_result();
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_witness_txo() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    //
    // normal
    //

    // send
    let _guard = stop_mining();
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );

    // check the recipient doesn't see the TXO yet + has one pending witness script
    let rcv_txos = rcv_party.db_txos();
    assert!(!rcv_txos.iter().any(|t| t.txid == txid));
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert_eq!(rcv_pending_witness_scripts.len(), 1);

    // sync recipient wallet
    rcv_party
        .wallet
        .sync(
            rcv_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();

    // check the recipient doesn't see the TXO yet + has one pending witness
    let rcv_txos = rcv_party.db_txos();
    assert!(!rcv_txos.iter().any(|t| t.txid == txid));
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert_eq!(rcv_pending_witness_scripts.len(), 1);

    // refresh the recipient to move the transfer to WaitingConfirmations
    rcv_party.refresh_all();
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );

    // check the recipient now sees the TXO yet as inexistent + pending witness
    let rcv_txos = rcv_party.db_txos();
    let rcv_witness_txos: Vec<database::entities::txo::Model> =
        rcv_txos.into_iter().filter(|t| t.txid == txid).collect();
    assert_eq!(rcv_witness_txos.len(), 1);
    let rcv_txo = rcv_witness_txos.first().unwrap();
    assert!(!rcv_txo.exists);
    assert!(rcv_txo.pending_witness);
    let rcv_outpoint = Outpoint {
        txid,
        vout: rcv_txo.vout,
    };

    // check the recipient still has the pending witness script
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert_eq!(rcv_pending_witness_scripts.len(), 1);

    // refresh the sender to move the transfer to WaitingConfirmations (broadcast)
    party.refresh_all();

    // sync recipient wallet
    rcv_party
        .wallet
        .sync(
            rcv_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();

    // check the recipient TXO now exists and is still pending witness
    let rcv_txo = rcv_party.db_txo(&rcv_outpoint).unwrap();
    assert!(rcv_txo.exists);
    assert!(rcv_txo.pending_witness);

    // check the recipient pending witness script has been deleted
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert!(rcv_pending_witness_scripts.is_empty());

    // mine + refresh the recipient to move the transfer to Settled
    drop(_guard);
    mine(false);
    rcv_party.refresh_all();
    party.refresh_all(); // so that change is spendable
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);

    // check the recipient TXO still exists and is no more pending witness
    let rcv_txo = rcv_party.db_txo(&rcv_outpoint).unwrap();
    assert!(rcv_txo.exists);
    assert!(!rcv_txo.pending_witness);

    //
    // donation
    //

    // new recipient wallet
    let mut rcv_party = get_empty_party!();

    // send (donation)
    let _guard = stop_mining();
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let OperationResult { txid, .. } = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap();
    assert!(!txid.is_empty());
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );

    // check the recipient doesn't see the TXO yet + has one pending witness script
    let rcv_txos = rcv_party.db_txos();
    assert!(!rcv_txos.iter().any(|t| t.txid == txid));
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert_eq!(rcv_pending_witness_scripts.len(), 1);

    // sync recipient wallet
    rcv_party
        .wallet
        .sync(
            rcv_party.online,
            SyncOptions {
                keychain: SyncKeychain::Colored,
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();

    // check the recipient now sees the TXO, as existent + pending witness
    let rcv_txos = rcv_party.db_txos();
    let rcv_witness_txos: Vec<_> = rcv_txos.into_iter().filter(|t| t.txid == txid).collect();
    assert_eq!(rcv_witness_txos.len(), 1);
    let rcv_txo = rcv_witness_txos.first().unwrap();
    assert!(rcv_txo.exists);
    assert!(rcv_txo.pending_witness);
    let rcv_outpoint = Outpoint {
        txid,
        vout: rcv_txo.vout,
    };

    // check pending witness script has been deleted
    let rcv_pending_witness_scripts = rcv_party.db_pending_witness_scripts();
    assert!(rcv_pending_witness_scripts.is_empty());

    // refresh to move the transfer to WaitingConfirmations
    rcv_party.refresh_all();
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );

    // check the TXO is still existent + pending witness
    let rcv_txo = rcv_party.db_txo(&rcv_outpoint).unwrap();
    assert!(rcv_txo.exists);
    assert!(rcv_txo.pending_witness);

    // refresh + mine to move the transfer to Settled
    party.refresh_all();
    drop(_guard);
    mine(false);
    rcv_party.refresh_all();
    let rcv_transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (rcv_transfer_data, _) = rcv_party.get_test_transfer_data(&rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);

    // check the TXO is still existent but not pending witness anymore
    let rcv_txo = rcv_party.db_txo(&rcv_outpoint).unwrap();
    assert!(rcv_txo.exists);
    assert!(!rcv_txo.pending_witness);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn blinded_change_failed_xfer() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party_1 = get_funded_noutxo_party!();
    let mut party_2 = get_funded_party!();

    // create 2 small UTXOs on wallet 1: 1 for issuance, 1 for change
    party_1.create_utxos(true, Some(2), Some(487), FEE_RATE, None);

    // issue
    let asset = party_1.issue_asset_nia(None);

    // send 1: 1 > 2 (no broadcast, fail instead)
    party_1.show_unspent_colorings("wallet 1: pre send 1");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + 1) as u64), // expire early so can fail
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // fail transfer on recipient side
    std::thread::sleep(Duration::from_secs(2));
    // send
    assert!(party_2.fail_transfers(Some(1), false, false).unwrap());
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // fail transfer on sender side
    assert!(party_1.fail_transfers(Some(2), false, false).unwrap());

    // send 2: 1 > 2 (complete)
    party_1.show_unspent_colorings("wallet 1: pre send 2");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // create another small UTXO on wallet 1 for blinded change
    party_1.create_utxos(false, Some(1), Some(487), FEE_RATE, None);

    // send 3: 1 > 2 (spend the change)
    party_1.show_unspent_colorings("wallet 1: pre send 3");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // send 4: 2 > 1 (spend the received allocation)
    party_2.show_unspent_colorings("wallet 2: pre send 4");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    party_1.show_unspent_colorings("wallet 1: final");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    party_2.show_unspent_colorings("wallet 2: final");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn blinded_change_send_begin_only() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party_1 = get_funded_noutxo_party!();
    let mut party_2 = get_funded_party!();

    // create 2 small UTXOs on wallet 1: 1 for issuance, 1 for change
    party_1.create_utxos(true, Some(2), Some(487), FEE_RATE, None);

    // issue
    let asset = party_1.issue_asset_nia(None);

    // send 1: 1 > 2 (send_begin only)
    party_1.show_unspent_colorings("wallet 1: pre send 1");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + 1) as u64), // expire early so can fail
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // fail transfer on recipient side
    std::thread::sleep(Duration::from_secs(2));
    // send (send_begin only)
    assert!(party_2.fail_transfers(Some(1), false, false).unwrap());
    let result = party_1.send_begin_result(&recipient_map).unwrap();
    assert!(!result.psbt.is_empty());
    // fail the initiated transfer to free up UTXOs for the next send
    assert!(
        party_1
            .fail_transfers(result.batch_transfer_idx, false, false)
            .unwrap()
    );

    // send 2: 1 > 2 (complete)
    party_1.show_unspent_colorings("wallet 1: pre send 2");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // send
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // create another small UTXO on wallet 1 for blinded change
    party_1.create_utxos(false, Some(1), Some(487), FEE_RATE, None);

    // send 3: 1 > 2 (spend the change)
    party_1.show_unspent_colorings("wallet 1: pre send 3");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // send 4: 2 > 1 (spend the received allocation)
    party_2.show_unspent_colorings("wallet 2: pre send 4");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    party_1.show_unspent_colorings("wallet 1: final");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    party_2.show_unspent_colorings("wallet 2: final");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn donation_recipient_nack() {
    initialize();

    let amount: u64 = 10;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_nia(None);

    // send 1: 1 > 2 (donation, fail from recipient side)
    party_1.show_unspent_colorings("wallet 1: pre send 1");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + 1) as u64), // expire early so can fail
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // fail transfer on recipient side
    std::thread::sleep(Duration::from_secs(2));
    // send
    assert!(party_2.fail_transfers(Some(1), false, false).unwrap());
    let txid = party_1
        .wallet
        .send(
            party_1.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());
    // manually NACK the transfer
    assert!(party_1.check_test_transfer_status_sender(&txid, TransferStatus::WaitingConfirmations));
    let proxy_client = get_proxy_client(None);
    proxy_client
        .post_ack(&receive_data.recipient_id, false)
        .unwrap();
    // settle on sender side
    mine(false);
    party_1.wait_for_refresh(None);
    assert!(party_1.check_test_transfer_status_sender(&txid, TransferStatus::Settled));
    assert!(
        party_2.check_test_transfer_status_recipient(
            &receive_data.recipient_id,
            TransferStatus::Failed
        )
    );

    // send 2: 1 > 2 (spend change)
    party_1.show_unspent_colorings("wallet 1: pre send 2");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_2.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);
    mine(false);
    party_2.wait_for_refresh(None);
    party_1.wait_for_refresh(None);

    // send 3: 2 > 1 (spend received allocation)
    party_2.show_unspent_colorings("wallet 2: pre send 3");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);
    mine(false);
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    party_1.show_unspent_colorings("wallet 1: final");
    println!(
        "balance 1: {:?}",
        party_1.get_asset_balance(&asset.asset_id)
    );
    party_2.show_unspent_colorings("wallet 2: final");
    println!(
        "balance 2: {:?}",
        party_2.get_asset_balance(&asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_end_without_send_begin() {
    initialize();

    let amount: u64 = 10;

    // wallets
    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue
    let asset = party_1.issue_asset_nia(None);

    // send begin on wallet 1 to create PSBT
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let unsigned_psbt = party_1.send_begin_result(&recipient_map).unwrap();

    let signed_psbt = party_1.wallet.sign_psbt(unsigned_psbt.psbt, None).unwrap();
    let psbt_txid = Psbt::from_str(&signed_psbt)
        .unwrap()
        .extract_tx()
        .unwrap()
        .compute_txid()
        .to_string();
    let result = party_2.wallet.send_end(party_2.online, signed_psbt);
    assert_matches!(result, Err(Error::UnknownTransfer { txid }) if txid == psbt_txid);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn allocations() {
    fn get_coloring_map(
        party: &SinglesigParty,
        unspents: &[Unspent],
    ) -> HashMap<DbTxo, Vec<DbColoring>> {
        let mut coloring_map: HashMap<DbTxo, Vec<DbColoring>> = HashMap::new();
        let db_txos = party.db_txos();
        let db_colorings = party.db_colorings();
        for u in unspents {
            let outpoint = &u.utxo.outpoint;
            let db_txo = db_txos
                .iter()
                .find(|t| t.txid == outpoint.txid && t.vout == outpoint.vout)
                .unwrap();
            let txo_colorings: Vec<&DbColoring> = db_colorings
                .iter()
                .filter(|c| c.txo_idx == db_txo.idx)
                .collect();
            coloring_map.insert(db_txo.clone(), txo_colorings.into_iter().cloned().collect());
        }
        coloring_map
    }

    fn check_allocations(
        party: &SinglesigParty,
        unspents_colorable: &[Unspent],
        amounts_user: &[u64],
        amounts_auto: &[u64],
        pending_xfer: bool,
    ) {
        let coloring_map = get_coloring_map(party, unspents_colorable);
        let db_asset_transfers = party.db_asset_transfers();
        let assignments_auto: Vec<_> = amounts_auto
            .iter()
            .map(|a| Assignment::Fungible(*a))
            .collect();
        // check input colorings
        let input_colorings: Vec<_> = coloring_map
            .values()
            .flatten()
            .filter(|c| c.r#type == ColoringType::Input)
            .collect();
        if pending_xfer {
            let assignments_user: Vec<_> = amounts_user
                .iter()
                .map(|a| Assignment::Fungible(*a))
                .collect();
            let assignments_all: Vec<_> = assignments_user
                .iter()
                .chain(assignments_auto.iter())
                .collect();
            assert_eq!(input_colorings.len(), assignments_all.len());
            for ass in assignments_all {
                input_colorings
                    .iter()
                    .find(|c| &c.assignment == ass)
                    .unwrap();
            }
            assert!(
                input_colorings
                    .iter()
                    .filter(|c| assignments_user.contains(&c.assignment))
                    .all(|c| db_asset_transfers
                        .iter()
                        .find(|a| a.idx == c.asset_transfer_idx)
                        .unwrap()
                        .user_driven)
            );
            assert!(
                input_colorings
                    .iter()
                    .filter(|c| assignments_auto.contains(&c.assignment))
                    .all(|c| !db_asset_transfers
                        .iter()
                        .find(|a| a.idx == c.asset_transfer_idx)
                        .unwrap()
                        .user_driven)
            );
        } else {
            assert_eq!(input_colorings.len(), 0);
        }
        // check change colorings
        let change_colorings: Vec<_> = coloring_map
            .values()
            .flatten()
            .filter(|c| c.r#type == ColoringType::Change)
            .collect();
        assert_eq!(change_colorings.len(), amounts_auto.len());
        for ass in &assignments_auto {
            change_colorings
                .iter()
                .find(|c| &c.assignment == ass)
                .unwrap();
        }
        assert!(
            change_colorings
                .iter()
                .filter(|c| assignments_auto.contains(&c.assignment))
                .all(|c| !db_asset_transfers
                    .iter()
                    .find(|a| a.idx == c.asset_transfer_idx)
                    .unwrap()
                    .user_driven)
        );
    }

    fn check_unspents(
        unspents_colorable: &[Unspent],
        amounts_user: &[u64],
        amounts_auto: &[u64],
        pending_xfer: bool,
    ) {
        let assignments_auto: Vec<_> = amounts_auto
            .iter()
            .map(|a| Assignment::Fungible(*a))
            .collect();
        // check input
        let inputs: Vec<_> = unspents_colorable
            .iter()
            .filter(|u| u.rgb_allocations.iter().all(|a| a.settled))
            .collect();
        if pending_xfer {
            assert_eq!(unspents_colorable.len(), 2);
            assert_eq!(inputs.len(), 1);
            let assignments_user: Vec<_> = amounts_user
                .iter()
                .map(|a| Assignment::Fungible(*a))
                .collect();
            let assignments_all: Vec<_> = assignments_user
                .iter()
                .chain(assignments_auto.iter())
                .collect();
            let assignments_input: Vec<_> = inputs[0]
                .rgb_allocations
                .iter()
                .map(|a| &a.assignment)
                .collect();
            assert_eq!(assignments_input.len(), assignments_all.len());
            for ass in assignments_all {
                assert!(assignments_input.contains(&ass));
            }
        } else {
            assert_eq!(unspents_colorable.len(), 1);
        }
        // check change
        let changes: Vec<_> = if pending_xfer {
            unspents_colorable
                .iter()
                .filter(|u| u.rgb_allocations.iter().any(|a| !a.settled))
                .collect()
        } else {
            unspents_colorable
                .iter()
                .filter(|u| u.rgb_allocations.iter().any(|a| a.settled))
                .collect()
        };
        assert_eq!(changes.len(), 1);
        let assignments_change: Vec<_> = changes[0]
            .rgb_allocations
            .iter()
            .map(|a| &a.assignment)
            .collect();
        assert_eq!(assignments_change.len(), assignments_auto.len());
        for ass in assignments_auto {
            assert!(assignments_change.contains(&&ass));
        }
    }

    initialize();

    let amount_1: u64 = 10;
    let amount_2: u64 = 20;
    let amount_3: u64 = 30;
    let amount_4: u64 = 40;
    let amount_5: u64 = 50;
    let amount_6: u64 = 60;
    let amounts_user = [amount_1, amount_2];
    let amounts_auto = [amount_3, amount_4, amount_5, amount_6];

    // wallets
    // - wallet 1 (standard)
    // - wallet 2 (1 UTXO, 6 max allocations per UTXO)
    // issue (allocations all on the same UTXO)
    let mut party_1 = get_funded_party!();
    let mut party_2 = offline_party!(get_test_wallet(true, Some(6))); // MAX_ALLOCATIONS_PER_UTXO = 6
    let online_2 = party_2.go_online(true, None);
    let mut party_2 = party!(party_2.wallet, online_2);
    fund_wallet(party_2.get_address());
    party_2.create_utxos(true, Some(1), None, FEE_RATE, None);

    let asset_1 = party_1.issue_asset_nia(None);
    let asset_2 = party_1.issue_asset_cfa(None, None);
    party_1.show_unspent_colorings("wallet 1 after issuance");

    // send to wallet 2, creating 6 allocations (4x asset 1, 2x asset 2) on the same UTXO
    let receive_data_1 = party_2.blind_receive();
    let receive_data_2 = party_2.blind_receive();
    let receive_data_3 = party_2.blind_receive();
    let receive_data_4 = party_2.blind_receive();
    let receive_data_5 = party_2.blind_receive();
    let receive_data_6 = party_2.blind_receive();
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount_1),
                    recipient_id: receive_data_1.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_2),
                    recipient_id: receive_data_2.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_3),
                    recipient_id: receive_data_3.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_4),
                    recipient_id: receive_data_4.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            asset_2.asset_id.clone(),
            vec![
                Recipient {
                    assignment: Assignment::Fungible(amount_5),
                    recipient_id: receive_data_5.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    assignment: Assignment::Fungible(amount_6),
                    recipient_id: receive_data_6.recipient_id.clone(),
                    witness_data: None,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
    ]);
    let txid = party_1.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // settle transfers
    party_2.refresh_all();
    party_1.refresh_all();
    mine(false);
    party_2.refresh_all();
    party_1.refresh_all();
    party_1.show_unspent_colorings("wallet 1 after setup send");
    party_2.show_unspent_colorings("wallet 2 after setup send");
    // check received allocation colorings
    let unspents_colorable = party_2.get_colorable_unspents_with_sync(false);
    let amounts_all = amounts_user
        .iter()
        .chain(amounts_auto.iter())
        .copied()
        .collect::<Vec<u64>>();
    check_allocations(&party_2, &unspents_colorable, &amounts_all, &[], false);

    // send the 2 smallest allocations from wallet 2
    let _guard = stop_mining();
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_1 + amount_2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // check allocation colorings
    // - main transition allocations have input colorings, user driven
    // - extra transition allocations have input + change colorings, not user driven
    party_2.show_unspent_colorings("wallet 2 after send (WaitingCounterparty)");
    party_1.show_unspent_colorings("wallet 1 after send (WaitingCounterparty)");
    let unspents_colorable = party_2.get_colorable_unspents_with_sync(false);
    print_unspents(
        &unspents_colorable,
        "wallet 2 unspents after send (WaitingCounterparty)",
    );
    check_allocations(
        &party_2,
        &unspents_colorable,
        &amounts_user,
        &amounts_auto,
        true,
    );
    // check unspent allocations (settled inputs, pending changes)
    check_unspents(&unspents_colorable, &amounts_user, &amounts_auto, true);

    // progress transfer to WaitingConfirmations
    party_1.refresh_all();
    party_2.refresh_all();
    // check allocation colorings (same as in WaitingCounterparty)
    party_2.show_unspent_colorings("wallet 2 after send (WaitingConfirmations)");
    party_1.show_unspent_colorings("wallet 1 after send (WaitingConfirmations)");
    let unspents_colorable = party_2.get_colorable_unspents_with_sync(false);
    print_unspents(
        &unspents_colorable,
        "wallet 2 unspents after send (WaitingConfirmations)",
    );
    check_allocations(
        &party_2,
        &unspents_colorable,
        &amounts_user,
        &amounts_auto,
        true,
    );
    // check unspent allocations (same as in WaitingCounterparty)
    check_unspents(&unspents_colorable, &amounts_user, &amounts_auto, true);

    // settle transfer
    drop(_guard);
    mine(false);
    party_1.refresh_all();
    party_2.refresh_all();
    // check allocation colorings (no input colorings, same change colorings)
    party_2.show_unspent_colorings("wallet 2 after send (Settled)");
    party_1.show_unspent_colorings("wallet 1 after send (Settled)");
    let unspents_colorable = party_2.get_colorable_unspents_with_sync(false);
    print_unspents(
        &unspents_colorable,
        "wallet 2 unspents after send (Settled)",
    );
    check_allocations(
        &party_2,
        &unspents_colorable,
        &amounts_user,
        &amounts_auto,
        false,
    );
    // check unspent allocations
    check_unspents(&unspents_colorable, &amounts_user, &amounts_auto, false);

    // send half of the smallest remaining allocation from wallet 2
    let receive_data = party_1.blind_receive();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_3 / 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());
    // check allocation colorings
    // - main transition allocations have input colorings, user driven
    // - extra transition allocations have input + change colorings, not user driven
    party_2.show_unspent_colorings("wallet 2 after 2nd send (WaitingCounterparty)");
    party_1.show_unspent_colorings("wallet 1 after 2nd send (WaitingCounterparty)");
    let amounts_input = [amount_3, amount_4, amount_5, amount_6];
    let amounts_change = [amount_3 / 2, amount_4, amount_5, amount_6];
    let unspents_colorable = party_2.get_colorable_unspents_with_sync(false);
    print_unspents(
        &unspents_colorable,
        "wallet 2 unspents after 2nd send (WaitingCounterparty)",
    );
    let coloring_map = get_coloring_map(&party_2, &unspents_colorable);
    let db_batch_transfers = party_2.db_batch_transfers();
    let db_asset_transfers = party_2.db_asset_transfers();
    // check input colorings
    let input_colorings: Vec<_> = coloring_map
        .values()
        .flatten()
        .filter(|c| c.r#type == ColoringType::Input)
        .collect();
    // - 4 colorings
    assert_eq!(input_colorings.len(), 4);
    for amt in amounts_input {
        input_colorings
            .iter()
            .find(|c| c.assignment == Assignment::Fungible(amt))
            .unwrap();
    }
    // - input for the main transition is user driven
    assert!(
        input_colorings
            .iter()
            .filter(|c| c.assignment == Assignment::Fungible(amount_3))
            .all(|c| db_asset_transfers
                .iter()
                .find(|a| a.idx == c.asset_transfer_idx)
                .unwrap()
                .user_driven)
    );
    // - other inputs are not user driven
    for amt in amounts_change {
        assert!(
            input_colorings
                .iter()
                .filter(|c| c.assignment == Assignment::Fungible(amt))
                .all(|c| !db_asset_transfers
                    .iter()
                    .find(|a| a.idx == c.asset_transfer_idx)
                    .unwrap()
                    .user_driven)
        );
    }
    // check change colorings
    let change_colorings: Vec<_> = coloring_map
        .values()
        .flatten()
        .filter(|c| c.r#type == ColoringType::Change)
        .filter(|c| {
            db_batch_transfers
                .iter()
                .find(|b| {
                    b.idx
                        == db_asset_transfers
                            .iter()
                            .find(|a| a.idx == c.asset_transfer_idx)
                            .unwrap()
                            .batch_transfer_idx
                })
                .unwrap()
                .waiting()
        })
        .collect();
    // - 4 pending ones
    assert_eq!(change_colorings.len(), 4);
    for amt in amounts_change {
        change_colorings
            .iter()
            .find(|c| c.assignment == Assignment::Fungible(amt))
            .unwrap();
    }
    // - change from the main transition is user driven
    assert!(
        change_colorings
            .iter()
            .filter(|c| c.assignment == Assignment::Fungible(amount_3 / 2))
            .all(|c| db_asset_transfers
                .iter()
                .find(|a| a.idx == c.asset_transfer_idx)
                .unwrap()
                .user_driven)
    );
    // - other changes are not user driven
    for amt in [amount_4, amount_5, amount_6] {
        assert!(
            change_colorings
                .iter()
                .filter(|c| c.assignment == Assignment::Fungible(amt))
                .all(|c| !db_asset_transfers
                    .iter()
                    .find(|a| a.idx == c.asset_transfer_idx)
                    .unwrap()
                    .user_driven)
        );
    }
}

// End-to-end RGB on P2WPKH: fund + create_utxos + issue NIA + receive + send.
// Exercises the OpretFirst commitment on a non-taproot output with both
// blinded and witness recipients.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn p2wpkh_send_receive_nia() {
    initialize();

    let amount_blind: u64 = 40;
    let amount_witness: u64 = 26;
    let total = amount_blind + amount_witness;

    let mut party = get_funded_party_p2wpkh();
    let mut rcv_party = get_funded_party_p2wpkh();

    let asset = party.issue_asset_nia(None);

    // two recipients on the P2WPKH receiver: one blinded, one witness
    let receive_blind = rcv_party.blind_receive();
    let receive_witness = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                assignment: Assignment::Fungible(amount_blind),
                recipient_id: receive_blind.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                assignment: Assignment::Fungible(amount_witness),
                recipient_id: receive_witness.recipient_id.clone(),
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

    // drive transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    // settled balance on both sides
    let balance_sender = party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_sender.settled, AMOUNT - total);
    let balance_receiver = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_receiver.settled, total);
}

// Cross-type RGB transfer (P2TR → P2WPKH) with a full send-back roundtrip.
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn cross_type_p2tr_to_p2wpkh() {
    initialize();

    let amount: u64 = 66;
    let amount_back: u64 = 10;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party_p2wpkh();

    let asset = party.issue_asset_nia(None);

    // P2TR → P2WPKH
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // drive transfers from WaitingCounterparty to Settled
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));

    let balance_sender = party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_sender.settled, AMOUNT - amount);
    let balance_receiver = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_receiver.settled, amount);

    // P2WPKH → P2TR (send some back to complete the roundtrip)
    let receive_back = party.blind_receive();
    let recipient_map_back = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount_back),
            recipient_id: receive_back.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_back = rcv_party.send_retry(&recipient_map_back);
    assert!(!txid_back.is_empty());

    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(None);
    mine(false);
    party.wait_for_refresh(None);
    rcv_party.wait_for_refresh(None);

    let balance_sender = party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_sender.settled, AMOUNT - amount + amount_back);
    let balance_receiver = rcv_party.get_asset_balance(&asset.asset_id);
    assert_eq!(balance_receiver.settled, amount - amount_back);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn unsafe_history_waits_for_safe_height() {
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
    assert!(party_1.check_test_transfer_status_sender(&txid_1, TransferStatus::Settled));

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

    // mine another block so txid_1 reaches 2 confirmations, then refresh to ACK
    force_mine_no_resume_when_alone(false);
    party_2.wait_for_refresh_raw(None, Some(&[receive_data_2.batch_transfer_idx]));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // sender sees the ACK and broadcasts txid_2
    party_1.wait_for_refresh(Some(&asset.asset_id));
    assert!(
        party_1.check_test_transfer_status_sender(&txid_2, TransferStatus::WaitingConfirmations)
    );

    // mine, sender (min_confirmations = 1) settles, recipient (min_confirmations = 2) still in WaitingConfirmations
    force_mine_no_resume_when_alone(false);
    party_1.wait_for_refresh(Some(&asset.asset_id));
    assert!(party_1.check_test_transfer_status_sender(&txid_2, TransferStatus::Settled));
    assert!(!party_2.refresh_all());
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // mine again so txid_2 reaches 2 confirmations, recipient settles
    force_mine_no_resume_when_alone(false);
    party_2.wait_for_refresh_raw(None, Some(&[receive_data_2.batch_transfer_idx]));
    assert!(party_2.check_test_transfer_status_recipient(
        &receive_data_2.recipient_id,
        TransferStatus::Settled
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let mut party = get_funded_party!();
    let asset = party.issue_asset_nia(None);
    let receive_data = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(AMOUNT),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);

    // begin does not update backup_info with dry_run=true
    let bak_info_before = party.db_backup_info();
    let _res = party
        .wallet
        .send_begin(
            party.online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
            true,
        )
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let bak_info_before = party.db_backup_info();
    let res = party
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
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(res.psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    party.wallet.send_end(party.online, signed_psbt).unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
