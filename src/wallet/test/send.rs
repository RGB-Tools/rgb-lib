use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);

    // send
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let txid = test_send(&mut wallet, &online, &recipient_map);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(!txid.is_empty());
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let tte_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    assert_eq!(tte_data.len(), 1);
    let ce = tte_data.first().unwrap();
    assert_eq!(ce.1.endpoint, PROXY_URL);
    assert!(ce.0.used);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, asset_transfer) = get_test_transfer_data(&wallet, &transfer);

    // ack is None
    assert_eq!(rcv_transfer.ack, None);
    assert_eq!(transfer.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer.amount, 0.to_string());
    assert_eq!(transfer.amount, amount.to_string());
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
    assert_eq!(transfer_data.created_at, transfer_data.updated_at);
    // expiration is create timestamp + expiration offset
    assert_eq!(
        rcv_transfer_data.expiration,
        Some(rcv_transfer_data.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        transfer_data.expiration,
        Some(transfer_data.created_at + DURATION_SEND_TRANSFER)
    );
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
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);

    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
    // ack is now true on the sender side
    assert_eq!(transfer.ack, Some(true));
    // amount is now set on the receiver side
    assert_eq!(rcv_transfer.amount, amount.to_string());
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer.asset_id, Some(asset.asset_id.clone()));
    // update timestamp has been updated
    let rcv_updated_at = rcv_transfer_data.updated_at;
    let updated_at = transfer_data.updated_at;
    assert!(rcv_updated_at > rcv_transfer_data.created_at);
    assert!(updated_at > transfer_data.created_at);

    // asset has been received correctly
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
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
    mine(false, false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    // update timestamp has been updated
    assert!(rcv_transfer_data.updated_at > rcv_updated_at);
    assert!(transfer_data.updated_at > updated_at);

    // change is unspent once transfer is Settled
    let unspents = test_list_unspents(&mut wallet, None, true);
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
    let receive_data_api_proto = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            transport_endpoints.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_api_proto.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let tte_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
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
    let consignment = wallet
        .rest_client
        .clone()
        .get_consignment(
            PROXY_URL_MOD_API,
            receive_data_api_proto.recipient_id.clone(),
        )
        .unwrap();
    assert!(consignment.error.is_some());
    let consignment = wallet
        .rest_client
        .clone()
        .get_consignment(
            PROXY_URL_MOD_PROTO,
            receive_data_api_proto.recipient_id.clone(),
        )
        .unwrap();
    assert!(consignment.error.is_some());
    let consignment = wallet
        .rest_client
        .clone()
        .get_consignment(PROXY_URL, receive_data_api_proto.recipient_id.clone())
        .unwrap();
    assert!(consignment.result.is_some());
    // settle transfer
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let rcv_transfer =
        get_test_transfer_recipient(&rcv_wallet, &receive_data_api_proto.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspents_color_count_after = unspents.iter().filter(|u| u.utxo.colorable).count();
    assert_eq!(unspents_color_count_after, unspents_color_count_before);

    // send, ignoring invalid or unreachable rpc transport endpoints and using a fee
    // rate that requires additional inputs be added to cover fee costs
    let transport_endpoints = vec![
        format!("rpc://{PROXY_HOST_MOD_PROTO}"),
        format!("rpc://127.6.6.6:7777/json-rpc"),
        format!("rpc://{PROXY_HOST}"),
    ];
    let receive_data_invalid_unreachable = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            transport_endpoints.clone().into_iter().skip(1).collect(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_invalid_unreachable.recipient_id.clone(),
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = wallet
        .send(
            online.clone(),
            recipient_map,
            false,
            7,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let tte_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
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
    let consignment = wallet
        .rest_client
        .clone()
        .get_consignment(
            PROXY_URL_MOD_PROTO,
            receive_data_invalid_unreachable.recipient_id.clone(),
        )
        .unwrap();
    assert!(consignment.error.is_some());
    let consignment = wallet
        .rest_client
        .clone()
        .get_consignment(PROXY_URL, receive_data_invalid_unreachable.recipient_id)
        .unwrap();
    assert!(consignment.result.is_some());
    // settle transfer
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspents_color_count_after = unspents.iter().filter(|u| u.utxo.colorable).count();
    assert_eq!(unspents_color_count_after, unspents_color_count_before - 2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn spend_all() {
    initialize();

    let file_str = "README.md";

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_blank = test_issue_asset_cfa(
        &mut wallet,
        &online,
        Some(&[AMOUNT * 2]),
        Some(file_str.to_string()),
    );

    // check both assets are allocated to the same UTXO
    let unspents = test_list_unspents(&mut wallet, None, true);
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
    assert!(allocation_asset_ids.contains(&asset_blank.asset_id));

    // send
    test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: AMOUNT,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfers, asset_transfers, _) = get_test_transfers_sender(&wallet, &txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = get_test_transfer_data(&wallet, transfer);
    assert_eq!(asset_transfers.len(), 2);
    let asset_transfer = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset.asset_id.clone()))
        .unwrap();
    let asset_blank_asset_transfer = asset_transfers
        .iter()
        .find(|a| a.asset_id == Some(asset_blank.asset_id.clone()))
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
    // asset_blank asset transfer is not user driven
    assert!(!asset_blank_asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = get_test_transfer_data(&wallet, transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after tx mining + refresh
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 1);
    let transfer = transfers_for_asset.first().unwrap();
    let (transfer_data, _) = get_test_transfer_data(&wallet, transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check the completely spent asset doesn't show up in unspents anymore
    let unspents = test_list_unspents(&mut wallet, None, true);
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset.asset_id.clone()))
    });
    assert!(!found);
    // check the blank asset shows up in unspents
    let unspents = test_list_unspents(&mut wallet, None, true);
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset_blank.asset_id.clone()))
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // transfer 1 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer.amount, amount_1.to_string());
    assert_eq!(transfer.amount, amount_1.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        AMOUNT - amount_1
    );

    //
    // 2nd transfer
    //

    // send
    let receive_data_2 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // transfer 2 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer.amount, amount_2.to_string());
    assert_eq!(transfer.amount, amount_2.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        AMOUNT - amount_1 - amount_2
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_blank_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;

    // wallets
    let (mut wallet_1, online_1) = get_funded_noutxo_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    test_create_utxos(&mut wallet_1, &online_1, false, Some(1), None, FEE_RATE);

    // issue
    let asset_nia = test_issue_asset_nia(&mut wallet_1, &online_1, None);
    let asset_cfa = test_issue_asset_cfa(&mut wallet_1, &online_1, Some(&[AMOUNT * 2]), None);

    // check both assets are allocated to the same UTXO
    let unspents = test_list_unspents(&mut wallet_1, None, true);
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
    show_unspent_colorings(&mut wallet_1, "wallet 1 after issuance");

    //
    // 1st transfer, asset_nia: wallet 1 > wallet 2
    //

    // send
    println!("\n=== send 1");
    let receive_data_1 = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid_1.is_empty());
    show_unspent_colorings(&mut wallet_1, "wallet 1 after send 1, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_nia.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_nia.asset_id), None);

    // transfer 1 checks
    let transfers_w1 = test_list_transfers(&wallet_1, Some(&asset_nia.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = test_list_transfers(&wallet_2, Some(&asset_nia.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.amount, amount_1);
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.amount, amount_1);
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = test_list_unspents(&mut wallet_1, None, true);
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
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_nia.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2);
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);
    // sender RGB state map
    let mut change_outpoint_set = BTreeSet::new();
    change_outpoint_set.insert(RgbOutpoint::from(change_utxo.clone()));

    //
    // 2nd transfer, asset_cfa (blank in 1st send): wallet 1 > wallet 2
    //

    // send
    let receive_data_2 = test_blind_receive(&wallet_2);
    println!("\n=== send 2");
    let recipient_map = HashMap::from([(
        asset_cfa.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid_2.is_empty());
    show_unspent_colorings(&mut wallet_1, "wallet 1 after send 2, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_cfa.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_cfa.asset_id), None);

    // transfer 2 checks
    let transfers_w2 = test_list_transfers(&wallet_2, Some(&asset_cfa.asset_id));
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = test_list_transfers(&wallet_1, Some(&asset_cfa.asset_id));
    let transfer_w1 = transfers_w1.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.amount, amount_2);
    assert_eq!(transfer_w1.kind, TransferKind::Send);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.amount, amount_2);
    assert_eq!(transfer_w2.kind, TransferKind::ReceiveBlind);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = test_list_unspents(&mut wallet_1, None, true);
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
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_nia.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2 - amount_2);
    assert_eq!(ca_a2.asset_id, Some(asset_cfa.asset_id.clone()));
    assert!(ca_a2.settled);
    // sender RGB state map
    let mut change_outpoint_set = BTreeSet::new();
    change_outpoint_set.insert(RgbOutpoint::from(change_utxo.clone()));
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
    let file_str = "README.md";

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // issue
    let asset_nia = test_issue_asset_nia(&mut wallet_1, &online_1, None);
    let asset_cfa = test_issue_asset_cfa(
        &mut wallet_1,
        &online_1,
        Some(&[AMOUNT * 2]),
        Some(file_str.to_string()),
    );

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_a20 = test_blind_receive(&wallet_2);
    let receive_data_a25 = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                recipient_id: receive_data_a20.recipient_id.clone(),
                witness_data: None,
                amount: amount_1a,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                recipient_id: receive_data_a25.recipient_id.clone(),
                witness_data: None,
                amount: amount_1b,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);

    // transfer 1 checks
    let (transfers_w1, _, _) = get_test_transfers_sender(&wallet_1, &txid_1);
    let transfers_for_asset_nia = transfers_w1.get(&asset_nia.asset_id).unwrap();
    let transfers_for_asset_cfa = transfers_w1.get(&asset_cfa.asset_id).unwrap();
    assert_eq!(transfers_for_asset_nia.len(), 1);
    assert_eq!(transfers_for_asset_cfa.len(), 1);
    let transfer_w1a = transfers_for_asset_nia.first().unwrap();
    let transfer_w1b = transfers_for_asset_cfa.first().unwrap();
    let transfer_w2a = get_test_transfer_recipient(&wallet_2, &receive_data_a20.recipient_id);
    let transfer_w2b = get_test_transfer_recipient(&wallet_2, &receive_data_a25.recipient_id);
    let (transfer_data_w1a, _) = get_test_transfer_data(&wallet_1, transfer_w1a);
    let (transfer_data_w1b, _) = get_test_transfer_data(&wallet_1, transfer_w1b);
    let (transfer_data_w2a, _) = get_test_transfer_data(&wallet_2, &transfer_w2a);
    let (transfer_data_w2b, _) = get_test_transfer_data(&wallet_2, &transfer_w2b);
    assert_eq!(transfer_w1a.amount, amount_1a.to_string());
    assert_eq!(transfer_w1b.amount, amount_1b.to_string());
    assert_eq!(transfer_w2a.amount, amount_1a.to_string());
    assert_eq!(transfer_w2b.amount, amount_1b.to_string());
    assert_eq!(transfer_data_w1a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w1b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet_1, None, true);
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
    assert_eq!(change_allocation_a.amount, AMOUNT - amount_1a);
    assert_eq!(change_allocation_b.amount, AMOUNT * 2 - amount_1b);

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_b20 = test_blind_receive(&wallet_3);
    let receive_data_b25 = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                recipient_id: receive_data_b20.recipient_id.clone(),
                witness_data: None,
                amount: amount_2a,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                recipient_id: receive_data_b25.recipient_id.clone(),
                witness_data: None,
                amount: amount_2b,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_2 = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);

    // transfer 2 checks
    let (transfers_w2, _, _) = get_test_transfers_sender(&wallet_2, &txid_2);
    let transfers_for_asset_nia = transfers_w2.get(&asset_nia.asset_id).unwrap();
    let transfers_for_asset_cfa = transfers_w2.get(&asset_cfa.asset_id).unwrap();
    assert_eq!(transfers_for_asset_nia.len(), 1);
    assert_eq!(transfers_for_asset_cfa.len(), 1);
    let transfer_w2a = transfers_for_asset_nia.first().unwrap();
    let transfer_w2b = transfers_for_asset_cfa.first().unwrap();
    let transfer_w3a = get_test_transfer_recipient(&wallet_3, &receive_data_b20.recipient_id);
    let transfer_w3b = get_test_transfer_recipient(&wallet_3, &receive_data_b25.recipient_id);
    let (transfer_data_w2a, _) = get_test_transfer_data(&wallet_2, transfer_w2a);
    let (transfer_data_w2b, _) = get_test_transfer_data(&wallet_2, transfer_w2b);
    let (transfer_data_w3a, _) = get_test_transfer_data(&wallet_3, &transfer_w3a);
    let (transfer_data_w3b, _) = get_test_transfer_data(&wallet_3, &transfer_w3b);
    assert_eq!(transfer_w2a.amount, amount_2a.to_string());
    assert_eq!(transfer_w2b.amount, amount_2b.to_string());
    assert_eq!(transfer_w3a.amount, amount_2a.to_string());
    assert_eq!(transfer_w3b.amount, amount_2b.to_string());
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3b.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet_2, None, true);
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
    assert_eq!(change_allocation_a.amount, amount_1a - amount_2a);
    assert_eq!(change_allocation_b.amount, amount_1b - amount_2b);

    // check CFA asset has the correct media after being received
    let cfa_assets = wallet_3
        .list_assets(vec![AssetSchema::Cfa])
        .unwrap()
        .cfa
        .unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let recv_asset = cfa_assets.first().unwrap();
    let dst_path = recv_asset.media.as_ref().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_uda_success() {
    initialize();

    let amount_1: u64 = 1;
    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_uda(
        &mut wallet_1,
        &online_1,
        Some(DETAILS),
        Some(file_str),
        vec![&image_str, file_str],
    );
    assert!(wallet_1
        .database
        .get_asset(asset.asset_id.clone())
        .unwrap()
        .unwrap()
        .media_idx
        .is_none());

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_1 = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);

    // transfer 1 checks
    let (transfer_w1, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let transfer_w2 = get_test_transfer_recipient(&wallet_2, &receive_data_1.recipient_id);
    let (transfer_data_w1, _) = get_test_transfer_data(&wallet_1, &transfer_w1);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w1.amount, amount_1.to_string());
    assert_eq!(transfer_w2.amount, amount_1.to_string());
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_2 = test_witness_receive(&mut wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    assert!(wallet_3
        .database
        .get_asset(asset.asset_id.clone())
        .unwrap()
        .unwrap()
        .media_idx
        .is_none());
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);

    // transfer 2 checks
    let transfer_w3 = get_test_transfer_recipient(&wallet_3, &receive_data_2.recipient_id);
    let (transfer_w2, _, _) = get_test_transfer_sender(&wallet_2, &txid_2);
    let (transfer_data_w3, _) = get_test_transfer_data(&wallet_3, &transfer_w3);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w3.amount, amount_1.to_string());
    assert_eq!(transfer_w2.amount, amount_1.to_string());
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    // check asset has been received correctly
    let uda_assets = wallet_3
        .list_assets(vec![AssetSchema::Uda])
        .unwrap()
        .uda
        .unwrap();
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
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    // check attachments
    let media = token.attachments.get(&0).unwrap();
    assert_eq!(media.mime, "image/png");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(image_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
    let dst_digest = Path::new(&dst_path).file_name().unwrap().to_string_lossy();
    assert_eq!(src_digest, dst_digest);
    let media = token.attachments.get(&1).unwrap();
    assert_eq!(media.mime, "text/plain");
    let dst_path = media.file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
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
    let file_str = "README.md";

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_cfa(&mut wallet_1, &online_1, None, Some(file_str.to_string()));

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_1 = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);

    // transfer 1 checks
    let (transfer_w1, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let transfer_w2 = get_test_transfer_recipient(&wallet_2, &receive_data_1.recipient_id);
    let (transfer_data_w1, _) = get_test_transfer_data(&wallet_1, &transfer_w1);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w1.amount, amount_1.to_string());
    assert_eq!(transfer_w2.amount, amount_1.to_string());
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet_1, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w1.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        AMOUNT - amount_1
    );

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let receive_data_2 = test_blind_receive(&wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);

    // transfer 2 checks
    let transfer_w3 = get_test_transfer_recipient(&wallet_3, &receive_data_2.recipient_id);
    let (transfer_w2, _, _) = get_test_transfer_sender(&wallet_2, &txid_2);
    let (transfer_data_w3, _) = get_test_transfer_data(&wallet_3, &transfer_w3);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w3.amount, amount_2.to_string());
    assert_eq!(transfer_w2.amount, amount_2.to_string());
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = test_list_unspents(&mut wallet_2, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w2.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        amount_1 - amount_2
    );
    // check asset has been received correctly
    let cfa_assets = wallet_3
        .list_assets(vec![AssetSchema::Cfa])
        .unwrap()
        .cfa
        .unwrap();
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
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check digest for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_digest = src_hash.to_string();
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount: amount_1,
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount_2,
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, asset_transfers, _) = get_test_transfers_sender(&wallet, &txid);
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
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

    // ack is None
    assert_eq!(rcv_transfer_1.ack, None);
    assert_eq!(rcv_transfer_2.ack, None);
    assert_eq!(transfer_1.ack, None);
    assert_eq!(transfer_2.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer_1.amount, 0.to_string());
    assert_eq!(rcv_transfer_2.amount, 0.to_string());
    assert_eq!(transfer_1.amount, amount_1.to_string());
    assert_eq!(transfer_2.amount, amount_2.to_string());
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
    assert_eq!(transfer_data_1.created_at, transfer_data_1.updated_at);
    assert_eq!(transfer_data_2.created_at, transfer_data_2.updated_at);
    // expiration is create timestamp + expiration offset
    assert_eq!(
        rcv_transfer_data_1.expiration,
        Some(rcv_transfer_data_1.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        rcv_transfer_data_2.expiration,
        Some(rcv_transfer_data_2.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        transfer_data_1.expiration,
        Some(transfer_data_1.created_at + DURATION_SEND_TRANSFER)
    );
    assert_eq!(
        transfer_data_2.expiration,
        Some(transfer_data_2.created_at + DURATION_SEND_TRANSFER)
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
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
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
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

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
    assert_eq!(rcv_transfer_1.amount, amount_1.to_string());
    assert_eq!(rcv_transfer_2.amount, amount_2.to_string());
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
    mine(false, false);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
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
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

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
    let unspents = test_list_unspents(&mut wallet, None, true);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset_1 = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_2 = wallet
        .issue_asset_cfa(
            online.clone(),
            s!("NAME2"),
            Some(DETAILS.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
        )
        .unwrap();

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let receive_data_2 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![Recipient {
                amount: amount_1,
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_2.asset_id.clone(),
            vec![Recipient {
                amount: amount_2,
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, asset_transfers, _) = get_test_transfers_sender(&wallet, &txid);
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
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

    // ack is None
    assert_eq!(rcv_transfer_1.ack, None);
    assert_eq!(rcv_transfer_2.ack, None);
    assert_eq!(transfer_1.ack, None);
    assert_eq!(transfer_2.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer_1.amount, 0.to_string());
    assert_eq!(rcv_transfer_2.amount, 0.to_string());
    assert_eq!(transfer_1.amount, amount_1.to_string());
    assert_eq!(transfer_2.amount, amount_2.to_string());
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
    assert_eq!(transfer_data_1.created_at, transfer_data_1.updated_at);
    assert_eq!(transfer_data_2.created_at, transfer_data_2.updated_at);
    // expiration is create timestamp + expiration offset
    assert_eq!(
        rcv_transfer_data_1.expiration,
        Some(rcv_transfer_data_1.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        rcv_transfer_data_2.expiration,
        Some(rcv_transfer_data_2.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        transfer_data_1.expiration,
        Some(transfer_data_1.created_at + DURATION_SEND_TRANSFER)
    );
    assert_eq!(
        transfer_data_2.expiration,
        Some(transfer_data_2.created_at + DURATION_SEND_TRANSFER)
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
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset_1.asset_id), None);

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, rcv_asset_transfer_1) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, rcv_asset_transfer_2) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

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
    assert_eq!(rcv_transfer_1.amount, amount_1.to_string());
    assert_eq!(rcv_transfer_2.amount, amount_2.to_string());
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
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
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
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset_1.asset_id), None);

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data_1, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer_1);
    let (rcv_transfer_data_2, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer_2);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let (transfer_data_1, _) = get_test_transfer_data(&wallet, transfer_1);
    let (transfer_data_2, _) = get_test_transfer_data(&wallet, transfer_2);

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
    let unspents = test_list_unspents(&mut wallet, None, true);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, rcv_online_2) = get_funded_wallet!();

    // issue
    let asset_a = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_b = test_issue_asset_nia(&mut wallet, &online, None);
    let _asset_c = test_issue_asset_nia(&mut wallet, &online, None);

    show_unspent_colorings(&mut wallet, "after issuances");

    // check each assets is allocated to a different UTXO
    let unspents = test_list_unspents(&mut wallet, None, true);
    let unspents_with_rgb_allocations = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty());
    assert_eq!(unspents_with_rgb_allocations.count(), 3);

    // blind
    let receive_data_a1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_a2 = test_blind_receive(&rcv_wallet_2);
    let receive_data_b1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_b2 = test_blind_receive(&rcv_wallet_2);

    // send multiple assets to multiple recipients
    let recipient_map = HashMap::from([
        (
            asset_a.asset_id.clone(),
            vec![
                Recipient {
                    recipient_id: receive_data_a1.recipient_id.clone(),
                    witness_data: None,
                    amount: amount_a1,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    recipient_id: receive_data_a2.recipient_id.clone(),
                    witness_data: None,
                    amount: amount_a2,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            asset_b.asset_id.clone(),
            vec![
                Recipient {
                    recipient_id: receive_data_b1.recipient_id.clone(),
                    witness_data: None,
                    amount: amount_b1,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    recipient_id: receive_data_b2.recipient_id.clone(),
                    witness_data: None,
                    amount: amount_b2,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
    ]);
    let txid = wallet
        .send(
            online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    show_unspent_colorings(&mut wallet, "after send");

    // check change UTXO has all the expected allocations
    let transfers_a = test_list_transfers(&wallet, Some(&asset_a.asset_id));
    let transfer_a = transfers_a.last().unwrap();
    let change_utxo = transfer_a.change_utxo.as_ref().unwrap();
    let unspents = test_list_unspents(&mut wallet, None, false);
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
    assert_eq!(allocation_a.unwrap().amount, AMOUNT - amount_a1 - amount_a2);
    assert_eq!(allocation_b.unwrap().amount, AMOUNT - amount_b1 - amount_b2);

    // take receiver transfers from WaitingCounterparty to Settled
    // (send_batch doesn't wait for recipient ACKs and proceeds to broadcast)
    wait_for_refresh(&mut rcv_wallet_1, &rcv_online_1, None, None);
    wait_for_refresh(&mut rcv_wallet_2, &rcv_online_2, None, None);
    test_list_transfers(&rcv_wallet_1, Some(&asset_a.asset_id));
    test_list_transfers(&rcv_wallet_1, Some(&asset_b.asset_id));
    test_list_transfers(&rcv_wallet_2, Some(&asset_a.asset_id));
    test_list_transfers(&rcv_wallet_2, Some(&asset_b.asset_id));
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet_1, &rcv_online_1, None, None);
    wait_for_refresh(&mut rcv_wallet_2, &rcv_online_2, None, None);
    test_list_transfers(&rcv_wallet_1, Some(&asset_a.asset_id));
    test_list_transfers(&rcv_wallet_1, Some(&asset_b.asset_id));
    test_list_transfers(&rcv_wallet_2, Some(&asset_a.asset_id));
    test_list_transfers(&rcv_wallet_2, Some(&asset_b.asset_id));

    show_unspent_colorings(&mut wallet, "after send, settled");
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_failed_blinded_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // 1st transfer
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            Some(60),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();
    assert!(!send_result.txid.is_empty());

    // try to send again and check the asset is not spendable
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: id }) if id == asset.asset_id)
    );

    // fail transfer so asset allocation can be spent again
    test_fail_transfers_single(&mut wallet, &online, send_result.batch_transfer_idx);

    // 2nd transfer using the same blinded UTXO
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDAlreadyUsed)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn ack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send with donation set to false
    let receive_data_1 = test_blind_receive(&rcv_wallet_1);
    let receive_data_2 = test_blind_receive(&rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: None,
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_id: receive_data_2.recipient_id.clone(),
                witness_data: None,
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // all transfers are in WaitingCounterparty status
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

    // ack from recipient 1 > its transfer status changes to WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet_1, &rcv_online_1, None, None);
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &receive_data_1.recipient_id,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));

    // ack from recipient 2 > its transfer status changes to WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet_2, &rcv_online_2, None, None);
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // now sender can broadcast and move on to WaitingConfirmations
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingConfirmations
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn nack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send with donation set to false
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // transfers are in WaitingCounterparty status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &receive_data.recipient_id,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));

    // manually NACK the transfer (consignment is valid so refreshing receiver would yield an ACK)
    rcv_wallet
        .rest_client
        .post_ack(PROXY_URL, receive_data.recipient_id, false)
        .unwrap();

    // refreshing sender transfer now has it fail
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::Failed
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn no_change_on_pending_send() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 32;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(3), None, FEE_RATE);
    assert_eq!(num_utxos_created, 3);

    // issue 1 + get its UTXO
    let asset_1 = test_issue_asset_nia(&mut wallet, &online, None);
    let unspents = test_list_unspents(&mut wallet, None, false);
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // issue 2
    let asset_2 = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT * 2]));

    show_unspent_colorings(&mut wallet, "before 1st send");
    // send asset_1
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_1.is_empty());

    // send asset_2 (send_1 in WaitingCounterparty)
    show_unspent_colorings(&mut wallet, "before 2nd send");
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let send_result = test_send_result(&mut wallet, &online, &recipient_map).unwrap();
    let txid_2 = send_result.txid;
    assert!(!txid_2.is_empty());
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));
    // fail send asset_2
    test_fail_transfers_single(&mut wallet, &online, send_result.batch_transfer_idx);

    // progress send_1 to WaitingConfirmations
    show_unspent_colorings(&mut wallet, "before refresh");
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset_1.asset_id), None);

    // send asset_2 (send_1 in WaitingConfirmations)
    show_unspent_colorings(&mut wallet, "before 3rd send");
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_3.is_empty());
    show_unspent_colorings(&mut wallet, "after 3rd send");
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    // blind
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            Some(60),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // invalid input (asset id)
    let recipient_map = HashMap::from([(
        s!("rgb1inexistent"),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // insufficient assets (amount too big)
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT + 1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientTotalAssets { asset_id: t }) if t == asset.asset_id)
    );

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
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
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown:{PROXY_HOST}")];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
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
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
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
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    let msg = s!("library supports at max 3 transport endpoints");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: no valid endpoints
    let transport_endpoints = vec![format!("rpc://{PROXY_HOST_MOD_API}")];
    let receive_data_te = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            transport_endpoints.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: AMOUNT / 2,
            recipient_id: receive_data_te.recipient_id,
            witness_data: None,
            transport_endpoints,
        }],
    )]);
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::NoValidTransportEndpoint)));

    // fee min
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: AMOUNT / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        0,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // duplicated recipient ID
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                amount: AMOUNT / 2,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                amount: AMOUNT / 3,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDDuplicated)));

    // amount 0
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: 0,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::InvalidAmountZero)));

    // blinded with witness data
    let receive_data_blinded = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_blinded.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    let details = "cannot provide witness data for a blinded recipient";
    assert!(matches!(result, Err(Error::InvalidRecipientData { details: m }) if m == details));

    // witness with no witness data
    let receive_data_witness = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_witness.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    let details = "missing witness data for a witness recipient";
    assert!(matches!(result, Err(Error::InvalidRecipientData { details: m }) if m == details));

    // output below dust limit
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data_witness.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 0,
                blinding: None,
            }),
            amount: AMOUNT / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // unsupported layer 1
    println!("setting MOCK_CHAIN_NET");
    *MOCK_CHAIN_NET.lock().unwrap() = Some(ChainNet::LiquidTestnet);
    let receive_data_liquid = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_liquid.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::InvalidRecipientNetwork)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_incoming_transfer_fail() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_noutxo_wallet!();
    test_create_utxos(&mut rcv_wallet, &rcv_online, false, Some(1), None, FEE_RATE);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    show_unspent_colorings(&mut wallet, "sender after 1st send, settled");
    show_unspent_colorings(&mut rcv_wallet, "receiver after 1st send, settled");

    //
    // 2nd transfer
    //

    // add a blind to the same UTXO
    let _receive_data_2 = test_blind_receive(&rcv_wallet);
    show_unspent_colorings(&mut rcv_wallet, "receiver after 2nd blind");

    // send from receiving wallet, 1st receive Settled, 2nd one still pending
    let receive_data = test_blind_receive(&wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    show_unspent_colorings(&mut wallet, "sender after 2nd send, WaitingCounterparty");
    show_unspent_colorings(
        &mut rcv_wallet,
        "receiver after 2nd send, WaitingCounterparty",
    );
    // check input allocation is blocked by pending receive
    let result = test_send_result(&mut rcv_wallet, &rcv_online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );

    // refresh on both wallets (no transfer status changes)
    assert!(!test_refresh_all(&mut rcv_wallet, &rcv_online));
    assert!(!test_refresh_asset(&mut wallet, &online, &asset.asset_id));
    // check input allocation is still blocked by pending receive
    let result = test_send_result(&mut rcv_wallet, &rcv_online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // issue asset
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // 1st send
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    show_unspent_colorings(&mut wallet, "sender after 1st send");

    // check change UTXO has exists = false and unspents list it
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    let unspents = test_list_unspents(&mut wallet, Some(&online), false);
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
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount / 2,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // check input allocation is blocked by pending send
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );

    // take transfer from WaitingCounterparty to WaitingConfirmations
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    // check input allocation is still blocked by pending send
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_transfer_input_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();
    test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // blind with sender wallet to create a pending transfer
    let _receive_data = test_blind_receive(&wallet);
    show_unspent_colorings(&mut wallet, "sender after blind");

    // send and check it fails as the issuance UTXO is "blocked" by the pending receive operation
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn already_used_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset to 3 UTXOs
    let asset = test_issue_asset_nia(
        &mut wallet,
        &online,
        Some(&[AMOUNT, AMOUNT * 2, AMOUNT * 3]),
    );

    // 1st transfer
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            Some(60),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // 2nd transfer using the same blinded UTXO
    let result = test_send_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::RecipientIDAlreadyUsed)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn cfa_blank_success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue NIA
    let asset_nia = test_issue_asset_nia(&mut wallet, &online, None);

    // issue CFA
    let _asset_cfa = test_issue_asset_cfa(&mut wallet, &online, None, None);

    let receive_data = test_blind_receive(&rcv_wallet);

    // try sending NIA
    let recipient_map = HashMap::from([(
        asset_nia.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(!result.unwrap().is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn psbt_rgb_consumer_success() {
    initialize();

    // create wallet with funds and no UTXOs
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO
    println!("utxo 1");
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // issue an NIA asset
    println!("issue 1");
    let asset_nia_a = test_issue_asset_nia(&mut wallet, &online, None);

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    let num_utxos_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // try to send it
    println!("send_begin 1");
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(!result.unwrap().is_empty());

    // issue one more NIA asset, should go to the same UTXO as the 1st issuance
    println!("issue 2");
    let asset_nia_b = test_issue_asset_nia(&mut wallet, &online, None);

    // try to send the second asset
    println!("send_begin 2");
    let receive_data_2 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_nia_b.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(!result.unwrap().is_empty());

    // exhaust allocations + issue 3rd asset, on a different UTXO
    println!("exhaust allocations on current UTXO");
    let new_allocation_count = MAX_ALLOCATIONS_PER_UTXO - 2;
    for _ in 0..new_allocation_count {
        let _receive_data = test_blind_receive(&wallet);
    }
    println!("issue 3");
    let asset_nia_c = test_issue_asset_nia(&mut wallet, &online, None);
    // fail transfers so 1st UTXO can be used as input
    test_fail_transfers_all(&mut wallet, &online);

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 3");
    let num_utxos_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // try to send the second asset to a recipient and the third to different one
    println!("send_begin 3");
    let receive_data_3a = test_blind_receive(&rcv_wallet);
    let receive_data_3b = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([
        (
            asset_nia_b.asset_id,
            vec![Recipient {
                amount: 1,
                recipient_id: receive_data_3a.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_nia_c.asset_id,
            vec![Recipient {
                amount: 1,
                recipient_id: receive_data_3b.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(!result.unwrap().is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_bitcoins() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send and drain the rest
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        &online,
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    test_drain_to_keep(&mut wallet, &online, &test_get_address(&mut rcv_wallet));

    // issue an NIA asset
    let asset_nia_a = test_issue_asset_nia(&mut wallet, &online, None);

    // send with no colorable UTXOs available as additional bitcoin inputs and no other funds
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 1);
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // create 1 UTXO for change (add funds, create UTXO, drain the rest)
    fund_wallet(test_get_address(&mut wallet));
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        &online,
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    test_drain_to_keep(&mut wallet, &online, &test_get_address(&mut rcv_wallet));

    // send works with no colorable UTXOs available as additional bitcoin inputs
    wait_for_unspents(&mut wallet, None, false, 2);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_allocations_fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        &online,
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);

    // issue an NIA asset
    let asset_nia_a = test_issue_asset_nia(&mut wallet, &online, None);

    // send with no colorable UTXOs available as change
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 2);
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    let num_utxos_created = test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // send works with no colorable UTXOs available as additional bitcoin inputs
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 3);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn insufficient_allocations_success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        &online,
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);

    // issue an NIA asset on the unspendable UTXO
    let asset_nia_a = test_issue_asset_nia(&mut wallet, &online, None);

    // create 2 more UTXOs, 1 for change + 1 as additional bitcoin input
    let num_utxos_created = test_create_utxos(&mut wallet, &online, false, Some(2), None, FEE_RATE);
    assert_eq!(num_utxos_created, 2);

    // send with 1 colorable UTXOs available as additional bitcoin input
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
    assert!(!result.unwrap().is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_to_oneself() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data = test_blind_receive(&wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status Settled after refreshes
    wait_for_refresh(&mut wallet, &online, None, Some(&[2, 3]));
    mine(false, false);
    wait_for_refresh(&mut wallet, &online, None, None);

    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    assert_eq!(batch_transfers.len(), 2);
    assert!(batch_transfers
        .iter()
        .all(|t| t.status == TransferStatus::Settled));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_received_back_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    //
    // 1st transfer: from issuer to recipient
    //

    // send
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // transfer 1 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_1.to_string());
    assert_eq!(transfer.amount, amount_1.to_string());

    let unspents = test_list_unspents(&mut wallet, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        AMOUNT - amount_1
    );

    //
    // 2nd transfer: from recipient back to issuer
    //

    // send
    let receive_data_2 = test_blind_receive(&wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut rcv_wallet, &rcv_online, &recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut wallet, &online, None, None);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet, &online, None, None);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, Some(&asset.asset_id), None);

    // transfer 2 checks
    let rcv_transfer = get_test_transfer_recipient(&wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&rcv_wallet, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&rcv_wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_2.to_string());
    assert_eq!(transfer.amount, amount_2.to_string());

    let unspents = test_list_unspents(&mut rcv_wallet, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        amount_1 - amount_2
    );

    //
    // 3rd transfer: from issuer to recipient once again (spend what was received back)
    //

    show_unspent_colorings(&mut wallet, "wallet before 3rd transfer");
    show_unspent_colorings(&mut rcv_wallet, "rcv_wallet before 3rd transfer");
    // send
    let receive_data_3 = test_blind_receive(&rcv_wallet);
    let change_3 = 5;
    let amount_3 = test_get_asset_balance(&wallet, &asset.asset_id).spendable - change_3;
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_3, // make sure to spend received transfer allocation
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid_3.is_empty());
    show_unspent_colorings(&mut wallet, "wallet after 3rd transfer");
    show_unspent_colorings(&mut rcv_wallet, "rcv_wallet after 3rd transfer");

    // take transfers from WaitingCounterparty to Settled
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // transfer 3 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_3);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_3.to_string());
    assert_eq!(transfer.amount, amount_3.to_string());

    let unspents = test_list_unspents(&mut wallet, None, true);
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(change_allocations.first().unwrap().amount, change_3);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn witness_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let receive_data_2 = test_witness_receive(&mut rcv_wallet);
    let receive_data_3 = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount,
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount * 2,
                recipient_id: receive_data_2.recipient_id,
                witness_data: Some(WitnessData {
                    amount_sat: 1200,
                    blinding: Some(7777),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount * 3,
                recipient_id: receive_data_3.recipient_id,
                witness_data: Some(WitnessData {
                    amount_sat: 1400,
                    blinding: Some(8888),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    test_create_utxos(&mut wallet, &online, false, None, None, FEE_RATE);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = get_test_asset_transfer(&wallet, batch_transfer.idx);
    let transfers = get_test_transfers(&wallet, asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
        assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
        // ack is now true on the sender side
        assert_eq!(transfer.ack, Some(true));
    }

    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveWitness);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(rcv_transfer.amount, amount.to_string());
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer.asset_id, Some(asset.asset_id.clone()));

    // asset has been received correctly
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
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
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = get_test_asset_transfer(&wallet, batch_transfer.idx);
    let transfers = get_test_transfers(&wallet, asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
        assert_eq!(transfer_data.status, TransferStatus::Settled);
        // change is unspent once transfer is Settled
        let unspents = test_list_unspents(&mut wallet, None, true);
        let change_unspent = unspents
            .into_iter()
            .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo);
        assert!(change_unspent.is_some());
    }
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);

    let balances = test_get_btc_balance(&mut rcv_wallet, &rcv_online);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset_1 = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_2 = test_issue_asset_nia(&mut wallet, &online, None);

    // send 1: check a transfer of multiple assets with multiple recepients works as expected
    println!("\nsend 1");
    let receive_data_1a = test_witness_receive(&mut rcv_wallet);
    let receive_data_1b = test_witness_receive(&mut rcv_wallet);
    let receive_data_2a = test_witness_receive(&mut rcv_wallet);
    let receive_data_2b = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![
                Recipient {
                    amount,
                    recipient_id: receive_data_1a.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_1a,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    amount: amount * 2,
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
                    amount: amount * 3,
                    recipient_id: receive_data_2a.recipient_id.clone(),
                    witness_data: Some(WitnessData {
                        amount_sat: btc_amount_2a,
                        blinding: None,
                    }),
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    amount: amount * 4,
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
    test_create_utxos(&mut wallet, &online, false, None, None, FEE_RATE);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // check receiver transfers
    let rcv_xfer_1a = get_test_transfer_recipient(&rcv_wallet, &receive_data_1a.recipient_id);
    let rcv_xfer_1b = get_test_transfer_recipient(&rcv_wallet, &receive_data_1b.recipient_id);
    let rcv_xfer_2a = get_test_transfer_recipient(&rcv_wallet, &receive_data_2a.recipient_id);
    let rcv_xfer_2b = get_test_transfer_recipient(&rcv_wallet, &receive_data_2b.recipient_id);
    let (rcv_xfer_data_1a, rcv_asset_xfer_1a) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_1a);
    let (rcv_xfer_data_1b, rcv_asset_xfer_1b) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_1b);
    let (rcv_xfer_data_2a, rcv_asset_xfer_2a) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_2a);
    let (rcv_xfer_data_2b, rcv_asset_xfer_2b) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_2b);
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
    assert_eq!(rcv_xfer_1a.amount, amount.to_string());
    assert_eq!(rcv_xfer_1b.amount, (amount * 2).to_string());
    assert_eq!(rcv_xfer_2a.amount, (amount * 3).to_string());
    assert_eq!(rcv_xfer_2b.amount, (amount * 4).to_string());
    assert_eq!(rcv_asset_xfer_1a.asset_id, Some(asset_1.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_1b.asset_id, Some(asset_1.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_2a.asset_id, Some(asset_2.asset_id.clone()));
    assert_eq!(rcv_asset_xfer_2b.asset_id, Some(asset_2.asset_id.clone()));
    // asset has been received correctly
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
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
    let tx_details = match rcv_wallet.indexer() {
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
        let transfer_vout = rcv_xfer.vout.unwrap() as u64;
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
    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = get_test_asset_transfers(&wallet, batch_transfer.idx);
    assert_eq!(asset_transfers.len(), 2);
    let transfers_1 = get_test_transfers(&wallet, asset_transfers.first().unwrap().idx);
    let transfers_2 = get_test_transfers(&wallet, asset_transfers.last().unwrap().idx);
    transfers_1.iter().chain(transfers_2.iter()).for_each(|t| {
        let (transfer_data, _) = get_test_transfer_data(&wallet, t);
        assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
    });

    // transfers progress to status Settled after tx mining + refresh
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // check receiver transfers
    let rcv_xfer_1a = get_test_transfer_recipient(&rcv_wallet, &receive_data_1a.recipient_id);
    let rcv_xfer_1b = get_test_transfer_recipient(&rcv_wallet, &receive_data_1b.recipient_id);
    let rcv_xfer_2a = get_test_transfer_recipient(&rcv_wallet, &receive_data_2a.recipient_id);
    let rcv_xfer_2b = get_test_transfer_recipient(&rcv_wallet, &receive_data_2b.recipient_id);
    let (rcv_xfer_data_1a, _) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_1a);
    let (rcv_xfer_data_1b, _) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_1b);
    let (rcv_xfer_data_2a, _) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_2a);
    let (rcv_xfer_data_2b, _) = get_test_transfer_data(&rcv_wallet, &rcv_xfer_2b);
    assert_eq!(rcv_xfer_data_1a.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_1b.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_2a.status, TransferStatus::Settled);
    assert_eq!(rcv_xfer_data_2b.status, TransferStatus::Settled);
    let rcv_balances = test_get_btc_balance(&mut rcv_wallet, &rcv_online);
    assert!(matches!(
        rcv_balances.colored,
        Balance {
            settled: 10200,
            future: 10200,
            spendable: 10200,
        }
    ));

    // check sender transfers
    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = get_test_asset_transfers(&wallet, batch_transfer.idx);
    assert_eq!(asset_transfers.len(), 2);
    let transfers_1 = get_test_transfers(&wallet, asset_transfers.first().unwrap().idx);
    let transfers_2 = get_test_transfers(&wallet, asset_transfers.last().unwrap().idx);
    transfers_1.iter().chain(transfers_2.iter()).for_each(|t| {
        let (transfer_data, _) = get_test_transfer_data(&wallet, t);
        assert_eq!(transfer_data.status, TransferStatus::Settled);
    });
    // asset has been received correctly
    let rcv_assets = test_list_assets(&rcv_wallet, &[]);
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
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount * 5,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // check receiver transfer
    let rcv_xfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_xfer_data, rcv_asset_xfer) = get_test_transfer_data(&rcv_wallet, &rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::WaitingCounterparty);
    assert_eq!(rcv_xfer.amount, 0.to_string());
    assert_eq!(rcv_asset_xfer.asset_id, None);
    // check asset balance: pending witness transfer not counted (no asset ID)
    let asset_1_balance = test_get_asset_balance(&rcv_wallet, &asset_1.asset_id);
    assert_eq!(
        asset_1_balance,
        Balance {
            settled: amount * 3,
            future: amount * 3,
            spendable: amount * 3,
        }
    );

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // check receiver transfer
    let rcv_xfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_xfer_data, rcv_asset_xfer) = get_test_transfer_data(&rcv_wallet, &rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::WaitingConfirmations);
    assert_eq!(rcv_xfer.amount, (amount * 5).to_string());
    assert_eq!(rcv_asset_xfer.asset_id, Some(asset_1.asset_id.clone()));
    // check asset balance: pending witness transfer counted (future)
    let asset_1_balance = test_get_asset_balance(&rcv_wallet, &asset_1.asset_id);
    assert_eq!(
        asset_1_balance,
        Balance {
            settled: amount * 3,
            future: amount * 8,
            spendable: amount * 3,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    // check receiver transfer
    let rcv_xfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_xfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_xfer);
    assert_eq!(rcv_xfer_data.status, TransferStatus::Settled);
    // check asset balance: pending witness transfer counted (settled + spendable as well)
    let asset_1_balance = test_get_asset_balance(&rcv_wallet, &asset_1.asset_id);
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
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet_1, &online_1, None);

    // send
    println!("send 1");
    let receive_data_1a = test_witness_receive(&mut wallet_2);
    let receive_data_1b = test_witness_receive(&mut wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount,
                recipient_id: receive_data_1a.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount * 2,
                recipient_id: receive_data_1b.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1200,
                    blinding: Some(7777),
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);

    println!("send 2");
    let receive_data_2 = test_witness_receive(&mut wallet_1);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: 77,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);

    println!("send 3");
    let receive_data_3 = test_witness_receive(&mut wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: 40,
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet_1, &online_1, &recipient_map);
    assert!(!txid.is_empty());

    // settle transfers
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, None, None);
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);

    // check transfers have settled
    let rcv_transfer = get_test_transfer_recipient(&wallet_2, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_2, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_1, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet_1, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn witness_fail_wrong_vout() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send
    let receive_data_1 = test_witness_receive(&mut rcv_wallet_1);
    let receive_data_2 = test_witness_receive(&mut rcv_wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount,
                recipient_id: receive_data_1.recipient_id.clone(),
                witness_data: Some(WitnessData {
                    amount_sat: 1000,
                    blinding: None,
                }),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount * 2,
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
    *MOCK_VOUT.lock().unwrap() = Some(2);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // transfers progress to status Failed after a refresh
    wait_for_refresh(&mut rcv_wallet_2, &rcv_online_2, None, None);
    wait_for_refresh(&mut rcv_wallet_1, &rcv_online_1, None, None);
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet_1, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet_1, &rcv_transfer);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    let batch_transfers = get_test_batch_transfers(&wallet, &txid);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = get_test_asset_transfer(&wallet, batch_transfer.idx);
    let transfers = get_test_transfers(&wallet, asset_transfer.idx);
    for transfer in transfers {
        let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
        assert_eq!(transfer_data.status, TransferStatus::Failed);
    }
    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveWitness);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Failed);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn _min_confirmations_common(
    wallet: &mut Wallet,
    online: &Online,
    rcv_wallet: &mut Wallet,
    rcv_online: &Online,
    esplora: bool,
) {
    let amount: u64 = 66;

    // 2 minimum confirmations
    println!("2 confirmations");
    let min_confirmations = 2;

    // issue
    let asset = test_issue_asset_nia(wallet, online, None);

    // avoid bitcoind sync issues
    stop_mining_when_alone();
    force_mine_no_resume_when_alone(esplora);

    // send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = wallet
        .send(
            online.clone(),
            recipient_map,
            false,
            FEE_RATE,
            min_confirmations,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    let rcv_transfer = get_test_transfer_recipient(rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (_, rcv_batch_transfer) = get_test_transfer_related(rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    let (_, batch_transfer) = get_test_transfer_related(wallet, &transfer);
    assert_eq!(rcv_batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    wait_for_refresh(wallet, online, Some(&asset.asset_id), None);

    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers remain in status WaitingConfirmations after a block is mined
    force_mine_no_resume_when_alone(esplora);
    assert!(!test_refresh_all(rcv_wallet, rcv_online));
    assert!(!test_refresh_asset(wallet, online, &asset.asset_id));

    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after a second block is mined
    force_mine_no_resume_when_alone(esplora);
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    wait_for_refresh(wallet, online, Some(&asset.asset_id), None);

    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // 0 minimum confirmations
    println!("0 confirmations");
    let min_confirmations = 0;

    // send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = wallet
        .send(
            online.clone(),
            recipient_map,
            false,
            FEE_RATE,
            min_confirmations,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    let rcv_transfer = get_test_transfer_recipient(rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (_, rcv_batch_transfer) = get_test_transfer_related(rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    let (_, batch_transfer) = get_test_transfer_related(wallet, &transfer);
    assert_eq!(rcv_batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    wait_for_refresh(wallet, online, Some(&asset.asset_id), None);

    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled before a block is mined
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    wait_for_refresh(wallet, online, Some(&asset.asset_id), None);

    let (rcv_transfer_data, _) = get_test_transfer_data(rcv_wallet, &rcv_transfer);
    let (transfer_data, _) = get_test_transfer_data(wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // spend received allocations
    let receive_data = test_blind_receive(wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount * 2,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(rcv_wallet, rcv_online, &recipient_map);
    assert!(!txid.is_empty());

    wait_for_refresh(wallet, online, None, None);
    wait_for_refresh(rcv_wallet, rcv_online, Some(&asset.asset_id), None);
    mine(esplora, true);
    wait_for_refresh(wallet, online, None, None);
    wait_for_refresh(rcv_wallet, rcv_online, Some(&asset.asset_id), None);

    let transfer = get_test_transfer_recipient(rcv_wallet, &receive_data.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(rcv_wallet, &transfer);
    let (rcv_transfer, _, _) = get_test_transfer_sender(wallet, &txid);
    let (rcv_transfer_data, _) = get_test_transfer_data(wallet, &rcv_transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn min_confirmations_electrum() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    _min_confirmations_common(&mut wallet, &online, &mut rcv_wallet, &rcv_online, false);
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn min_confirmations_esplora() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!(ESPLORA_URL.to_string());
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!(ESPLORA_URL.to_string());

    _min_confirmations_common(&mut wallet, &online, &mut rcv_wallet, &rcv_online, true);
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn spend_double_receive() {
    initialize();

    let amount_1 = 100;
    let amount_2 = 200;

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();
    // create bigger UTXOs for wallet_2 so a single one can support a witness transfer
    let (mut wallet_2, online_2) = get_funded_noutxo_wallet!();
    let created = test_create_utxos(&mut wallet_2, &online_2, false, None, Some(5000), FEE_RATE);
    assert_eq!(created, UTXO_NUM);

    // issue
    println!("issue");
    let asset = test_issue_asset_nia(&mut wallet_1, &online_1, None);

    // send a first time 1->2 (blind)
    println!("send blind 1->2");
    let receive_data = test_blind_receive(&wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = wallet_1
        .send(
            online_1.clone(),
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid_1.is_empty());
    // settle transfer
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);
    // check transfer status
    let rcv_transfer = get_test_transfer_recipient(&wallet_2, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_2, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet_1, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // send a second time 1->2 (witness, so the 2 allocations can't be on the same UTXO)
    println!("send witness 1->2");
    let receive_data = test_witness_receive(&mut wallet_2);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: Some(777),
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = wallet_1
        .send(
            online_1.clone(),
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid_2.is_empty());
    // settle transfer
    mine(false, false);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset.asset_id), None);
    // check transfer status
    let rcv_transfer = get_test_transfer_recipient(&wallet_2, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_2, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_1, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&wallet_1, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check wallet_2 has the 2 expected allocations
    let unspents = test_list_unspents(&mut wallet_2, None, true);
    let asset_unspents: Vec<&Unspent> = unspents
        .iter()
        .filter(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset.asset_id.clone()))
        })
        .collect();
    assert_eq!(asset_unspents.len(), 2);
    assert!(asset_unspents
        .first()
        .unwrap()
        .rgb_allocations
        .iter()
        .any(|a| a.amount == amount_1));
    assert!(asset_unspents
        .last()
        .unwrap()
        .rgb_allocations
        .iter()
        .any(|a| a.amount == amount_2));

    // send 2->3, manually selecting the 1st allocation (blind, amount_1) only
    println!("send witness 2->3");
    let receive_data = test_witness_receive(&mut wallet_3);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1, // amount of the 1st received allocation
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // manually set the input unspents to the UTXO of the 1st allocation
    let db_data = wallet_2.database.get_db_data(false).unwrap();
    let utxos = wallet_2
        .database
        .get_unspent_txos(db_data.txos.clone())
        .unwrap();
    let mut input_unspents = wallet_2
        .database
        .get_rgb_allocations(
            utxos,
            Some(db_data.colorings.clone()),
            Some(db_data.batch_transfers.clone()),
            Some(db_data.asset_transfers.clone()),
        )
        .unwrap();
    input_unspents.retain(|u| {
        !u.rgb_allocations.is_empty() && u.rgb_allocations.iter().all(|a| a.amount == amount_1)
    });
    assert_eq!(input_unspents.len(), 1);
    println!("setting MOCK_INPUT_UNSPENTS");
    MOCK_INPUT_UNSPENTS
        .lock()
        .unwrap()
        .push(input_unspents.first().unwrap().clone());
    // send (will use the manually-selected input unspent)
    let txid_3 = test_send(&mut wallet_2, &online_2, &recipient_map);
    assert!(!txid_3.is_empty());
    // settle transfer
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut wallet_3, &online_3, None, None);
    wait_for_refresh(&mut wallet_2, &online_2, Some(&asset.asset_id), None);
    // check transfer status
    let rcv_transfer = get_test_transfer_recipient(&wallet_3, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet_3, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet_2, &txid_3);
    let (transfer_data, _) = get_test_transfer_data(&wallet_2, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // check final balances
    let balance_1 = test_get_asset_balance(&wallet_1, &asset.asset_id);
    let balance_2 = test_get_asset_balance(&wallet_2, &asset.asset_id);
    let balance_3 = test_get_asset_balance(&wallet_3, &asset.asset_id);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue (allocations not sorted)
    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&amounts));

    // send, spending the 111 and 222 allocations
    println!("\nsend 1");
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());
    // settle transfers
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, Some(&asset.asset_id), None);

    // check the intended UTXOs have been used
    let unspents = list_test_unspents(&mut wallet, "after send");
    let allocations: Vec<&RgbAllocation> =
        unspents.iter().flat_map(|e| &e.rgb_allocations).collect();
    let mut cur_amounts: Vec<u64> = allocations.iter().map(|a| a.amount).collect();
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
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_noutxo_wallet!();

    // issue
    let asset_a = test_issue_asset_nia(&mut wallet_1, &online_1, None);

    // send
    let receive_data_1 = test_witness_receive(&mut wallet_2);
    let recipient_map_1 = HashMap::from([(
        asset_a.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet_1, &online_1, &recipient_map_1);
    assert!(!txid_1.is_empty());

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut wallet_2, &online_2, None, None);
    let transfer_1_recv = get_test_transfer_recipient(&wallet_2, &receive_data_1.recipient_id);
    let (transfer_1_recv_data, _) = get_test_transfer_data(&wallet_2, &transfer_1_recv);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_a.asset_id), None);
    let (transfer_1_send, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let (transfer_1_send_data, _) = get_test_transfer_data(&wallet_1, &transfer_1_send);
    assert_eq!(
        transfer_1_recv_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(
        transfer_1_send_data.status,
        TransferStatus::WaitingConfirmations
    );

    // mine and refresh the sender wallet only (receiver transfer still WaitingConfirmations)
    mine(false, false);
    wait_for_refresh(&mut wallet_1, &online_1, Some(&asset_a.asset_id), None);

    // sync DB TXOs for the receiver wallet
    test_create_utxos_begin_result(&mut wallet_2, &online_2, false, None, None, FEE_RATE).unwrap();

    // make sure the witness receive UTXO is available
    assert!(test_list_unspents(&mut wallet_2, Some(&online_2), false).len() > 1);

    // issue an asset on the witness receive UTXO
    let asset_b = test_issue_asset_nia(&mut wallet_2, &online_2, None);

    // spending the witness receive UTXO should fail
    let receive_data_2 = test_witness_receive(&mut wallet_1);
    let recipient_map_2 = HashMap::from([(
        asset_b.asset_id.clone(),
        vec![Recipient {
            amount: amount * 2,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_create_utxos(&mut wallet_2, &online_2, false, Some(2), None, FEE_RATE);
    let result = test_send_result(&mut wallet_2, &online_2, &recipient_map_2);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: ref id })
            if id == &asset_b.asset_id )
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn rgb_change_on_btc_change() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    test_create_utxos(&mut wallet, &online, false, Some(1), None, FEE_RATE);
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // send with no available colorable UTXOs (need to allocate change to BTC change UTXO)
    let receive_data = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // RGB change has been allocated to the same UTXO as the BTC change (exists = false)
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    let unspents = test_list_unspents(&mut wallet, None, false);
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
    assert_eq!(allocation.amount, AMOUNT - amount);

    // transfers progress to status WaitingConfirmations after a refresh
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wait_for_refresh(&mut wallet, &online, None, None);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);

    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // transfers progress to status Settled after tx mining + refresh
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    wait_for_refresh(&mut wallet, &online, None, None);

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
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
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _) = get_empty_wallet!();

    // create 1 UTXO
    let size = Some(UTXO_SIZE * 2);
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(1), size, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT]));

    // send
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: UTXO_SIZE as u64,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    show_unspent_colorings(&mut wallet, "after send (WaitingCounterparty)");

    // 1 UTXO being spent, 1 UTXO with exists = false
    // trying to get an UTXO for a blind receive should fail
    let result = test_blind_receive_result(&wallet);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    // trying to issue an asset should fail
    let result = test_issue_asset_nia_result(&mut wallet, &online, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    let result = test_issue_asset_cfa_result(&mut wallet, &online, None, None);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    let result = test_issue_asset_uda_result(&mut wallet, &online, None, None, vec![]);
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
    // trying to create 1 UTXO with up_to = true should create 1
    let num_utxos_created = test_create_utxos(&mut wallet, &online, true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // 1 UTXO being spent, 1 UTXO with exists = false, 1 new UTXO
    // issuing an asset should now succeed
    let asset_2 = test_issue_asset_nia(&mut wallet, &online, Some(&[AMOUNT * 2]));

    show_unspent_colorings(&mut wallet, "after 2nd issue");

    // 1 UTXO being spent, 1 UTXO with exists = false, 1 UTXO with an allocated asset
    // trying to send more BTC than what's available in the UTXO being spent should fail
    let receive_data = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat: UTXO_SIZE as u64,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = test_send_begin_result(&mut wallet, &online, &recipient_map);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // prepare transfer data
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        (1..=5)
            .map(|_| {
                let receive_data = test_witness_receive(&mut rcv_wallet);
                Recipient {
                    amount,
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
    let psbt_str = wallet
        .send_begin(
            online.clone(),
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let psbt = Psbt::from_str(&psbt_str).unwrap();
    let fee = psbt.fee().unwrap().to_sat();
    assert_eq!(fee, 510);

    // actual send
    let txid = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap()
        .txid;
    assert!(!txid.is_empty());

    // ACK transfer
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, None);
    // broadcast tx
    assert!(test_refresh_asset(&mut wallet, &online, &asset.asset_id));
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn _max_fee_exceeded_common(
    asset_id: &str,
    wallet: &mut Wallet,
    online: &Online,
    rcv_wallet: &mut Wallet,
    rcv_online: &Online,
    transfer_idx: i32,
) {
    let fee_rate = 20000;
    let amount = AMOUNT_SMALL;
    let amount_sat: u64 = 698;

    // get a lot of funds
    (0..9).for_each(|_| fund_wallet(test_get_address(wallet)));
    test_create_utxos(
        wallet,
        online,
        false,
        Some(20),
        Some(u32::MAX / 100),
        FEE_RATE,
    );

    // prepare transfer data
    let receive_data = test_witness_receive(rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_id.to_string(),
        vec![Recipient {
            amount,
            recipient_id: receive_data.recipient_id,
            witness_data: Some(WitnessData {
                amount_sat,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);

    // send
    let send_result = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap();
    assert!(!send_result.txid.is_empty());

    // ACK transfer
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    // broadcast tx
    let result = test_refresh_result(wallet, online, None, &[]).unwrap();
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
    test_fail_transfers_single(wallet, online, send_result.batch_transfer_idx);
    let result = test_refresh_result(wallet, online, None, &[]).unwrap();
    assert_eq!(result, HashMap::new());
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn max_fee_exceeded_electrum() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    _max_fee_exceeded_common(
        &asset.asset_id,
        &mut wallet,
        &online,
        &mut rcv_wallet,
        &rcv_online,
        2,
    );
}

#[cfg(feature = "esplora")]
#[test]
#[serial]
fn max_fee_exceeded_esplora() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!(ESPLORA_URL.to_string());
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!(ESPLORA_URL.to_string());

    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    _max_fee_exceeded_common(
        &asset.asset_id,
        &mut wallet,
        &online,
        &mut rcv_wallet,
        &rcv_online,
        2,
    );
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn _min_relay_fee_common(
    asset_id: &str,
    wallet: &mut Wallet,
    online: &Online,
    rcv_wallet: &mut Wallet,
    rcv_online: &Online,
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
                let receive_data = test_witness_receive(rcv_wallet);
                Recipient {
                    amount,
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
    *MOCK_CHECK_FEE_RATE.lock().unwrap() = vec![true, true];
    let psbt_str = wallet
        .send_begin(
            online.clone(),
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let psbt = Psbt::from_str(&psbt_str).unwrap();
    let fee = psbt.fee().unwrap().to_sat();
    assert_eq!(fee, 0);

    // actual send
    println!("setting MOCK_CHECK_FEE_RATE");
    *MOCK_CHECK_FEE_RATE.lock().unwrap() = vec![true, true];
    let send_result = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            fee_rate,
            MIN_CONFIRMATIONS,
            false,
        )
        .unwrap();
    assert!(!send_result.txid.is_empty());

    // ACK transfer
    wait_for_refresh(rcv_wallet, rcv_online, None, None);
    // broadcast tx
    let result = test_refresh_result(wallet, online, None, &[]).unwrap();
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
    test_fail_transfers_single(wallet, online, send_result.batch_transfer_idx);
    let result = test_refresh_result(wallet, online, None, &[]).unwrap();
    assert_eq!(result, HashMap::new());
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn min_relay_fee_electrum() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    _min_relay_fee_common(
        &asset.asset_id,
        &mut wallet,
        &online,
        &mut rcv_wallet,
        &rcv_online,
        2,
    );
}

#[cfg(feature = "esplora")]
#[test]
#[serial]
fn min_relay_fee_esplora() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!(ESPLORA_URL.to_string());
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!(ESPLORA_URL.to_string());

    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    _min_relay_fee_common(
        &asset.asset_id,
        &mut wallet,
        &online,
        &mut rcv_wallet,
        &rcv_online,
        2,
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn script_buf_to_from_recipient_id() {
    initialize();

    // wallets
    let (mut wallet, _online) = get_funded_wallet!();

    // get a script bug from an address (witness receive)
    let address_str = test_get_address(&mut wallet);
    let script_buf = wallet.get_script_pubkey(&address_str).unwrap();

    // recipient ID from script buf
    let recipient_id = recipient_id_from_script_buf(script_buf.clone(), BitcoinNetwork::Regtest);

    // script buf from recipient ID Some
    let script_from_recipient = script_buf_from_recipient_id(recipient_id).unwrap();

    // checks
    assert!(script_from_recipient.is_some());
    assert_eq!(script_from_recipient.unwrap(), script_buf);

    // script buf from recipient ID None (blinded)
    let receive_data = test_blind_receive(&wallet);
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
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue (2 allocations, 1 per send)
    let asset = test_issue_asset_nia(&mut wallet, &online, Some(&[100, 200]));
    let transfers = test_list_transfers(&wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);

    // send (blinded) skipping sync
    let receive_data_1 = test_blind_receive(&rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_1.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap()
        .txid;
    assert!(!txid_1.is_empty());

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);

    // amount
    assert_eq!(rcv_transfer.amount, 0.to_string());
    assert_eq!(transfer.amount, amount.to_string());
    // status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // send (witness) skipping sync
    let receive_data_2 = test_witness_receive(&mut rcv_wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_2.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap()
        .txid;
    assert!(!txid_2.is_empty());

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);

    // amount
    assert_eq!(rcv_transfer.amount, 0.to_string());
    assert_eq!(transfer.amount, amount.to_string());
    // status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // settle transfers
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, Some(&[1, 2]));
    wait_for_refresh(&mut wallet, &online, None, Some(&[2, 3]));
    mine(false, false);
    wait_for_refresh(&mut rcv_wallet, &rcv_online, None, Some(&[1, 2]));
    wait_for_refresh(&mut wallet, &online, None, Some(&[2, 3]));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_1,
        TransferStatus::Settled
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid_2,
        TransferStatus::Settled
    ));

    // send to oneself (witness) skipping sync
    let receive_data_3 = test_witness_receive(&mut wallet);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_id: receive_data_3.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = wallet
        .send(
            online.clone(),
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            true,
        )
        .unwrap()
        .txid;
    assert!(!txid_3.is_empty());

    // transfers are in WaitingCounterparty
    let rcv_transfer = get_test_transfer_recipient(&wallet, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_3);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer.amount, 0.to_string());
    assert_eq!(transfer.amount, amount.to_string());
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);

    // refresh skipping sync
    wallet.refresh(online.clone(), None, vec![], true).unwrap();

    // transfers are now in WaitingConfirmations
    let rcv_transfer = get_test_transfer_recipient(&wallet, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet, &rcv_transfer);
    let batch_transfers = get_test_batch_transfers(&wallet, &txid_3);
    let batch_transfer = batch_transfers.iter().find(|t| t.idx == 5).unwrap();
    let asset_transfer = get_test_asset_transfer(&wallet, batch_transfer.idx);
    let transfers = get_test_transfers(&wallet, asset_transfer.idx);
    let transfer = transfers.first().unwrap();
    let (transfer_data, _) = get_test_transfer_data(&wallet, transfer);
    assert_eq!(rcv_transfer.amount, amount.to_string());
    assert_eq!(transfer.amount, amount.to_string());
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations,
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);

    // mine and refresh skipping sync > cannot refresh ReceiveWitness transfer as a sync is needed
    mine(false, false);
    let result = wallet.refresh(online.clone(), None, vec![], true).unwrap();
    assert!(result
        .iter()
        .any(|(i, rt)| *i == 4 && rt.failure == Some(Error::SyncNeeded)));
    show_unspent_colorings(&mut wallet, "after refresh 2");

    // Send transfer is now settled
    let batch_transfers = get_test_batch_transfers(&wallet, &txid_3);
    let batch_transfer = batch_transfers.iter().find(|t| t.idx == 5).unwrap();
    let asset_transfer = get_test_asset_transfer(&wallet, batch_transfer.idx);
    let transfers = get_test_transfers(&wallet, asset_transfer.idx);
    let transfer = transfers.first().unwrap();
    let (transfer_data, _) = get_test_transfer_data(&wallet, transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // sync and refresh again (still skipping sync) > ReceiveWitness transfer now refreshes + new UTXO appears
    wallet.sync(online.clone()).unwrap();
    wallet.refresh(online.clone(), None, vec![], true).unwrap();
    show_unspent_colorings(&mut wallet, "after refresh 3");

    // ReceiveWitness transfer is now settled as well
    let rcv_transfer = get_test_transfer_recipient(&wallet, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet, &rcv_transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled,);
}
