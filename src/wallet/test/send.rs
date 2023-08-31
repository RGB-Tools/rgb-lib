const TINY_BTC_AMOUNT: u32 = 294;

use std::collections::BTreeSet;

use super::*;

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let transfers = wallet.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);

    // send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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

    stop_mining();

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
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
    let rcv_assets = rcv_wallet.list_assets(vec![]).unwrap();
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
    mine(true);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

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
    let unspents = wallet.list_unspents(None, true).unwrap();
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_api_proto.recipient_id).unwrap(),
            ),
            transport_endpoints,
        }],
    )]);
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    let rcv_transfer =
        get_test_transfer_recipient(&rcv_wallet, &receive_data_api_proto.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = wallet.list_unspents(None, false).unwrap();
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_invalid_unreachable.recipient_id).unwrap(),
            ),
            transport_endpoints,
        }],
    )]);
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspents_color_count_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let txid = wallet
        .send(online.clone(), recipient_map, false, 5.0, MIN_CONFIRMATIONS)
        .unwrap();
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
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(true);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id), vec![])
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspents_color_count_after = unspents.iter().filter(|u| u.utxo.colorable).count();
    assert_eq!(unspents_color_count_after, unspents_color_count_before - 1);
}

#[test]
fn spend_all() {
    initialize();

    let file_str = "README.md";

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_blank = wallet
        .issue_asset_cfa(
            online.clone(),
            s!("NAME2"),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            Some(file_str.to_string()),
        )
        .unwrap();

    // check both assets are allocated to the same UTXO
    let unspents = wallet.list_unspents(None, true).unwrap();
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
    test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: AMOUNT,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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

    stop_mining();

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
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
    mine(true);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()), vec![])
        .unwrap();

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
    let unspents = wallet.list_unspents(None, true).unwrap();
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset.asset_id.clone()))
    });
    assert!(!found);
    // check the blank asset shows up in unspents
    let unspents = wallet.list_unspents(None, true).unwrap();
    let found = unspents.iter().any(|u| {
        u.rgb_allocations
            .iter()
            .any(|a| a.asset_id == Some(asset_blank.asset_id.clone()))
    });
    assert!(found);
}

#[test]
fn send_twice_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // transfer 1 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer.amount, amount_1.to_string());
    assert_eq!(transfer.amount, amount_1.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = wallet.list_unspents(None, true).unwrap();
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
    let receive_data_2 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id), vec![])
        .unwrap();

    // transfer 2 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer.amount, amount_2.to_string());
    assert_eq!(transfer.amount, amount_2.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = wallet.list_unspents(None, true).unwrap();
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

#[test]
fn send_blank_success() {
    initialize();

    fn check_state_map_asset_amount(asset_state_map: &BTreeMap<Opout, TypedState>, amount: u64) {
        for typed_state in asset_state_map.values() {
            if *typed_state == TypedState::Amount(amount) {
                return;
            }
        }
        panic!("unexpected");
    }

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;
    let file_str = "README.md";

    // wallets
    let (mut wallet_1, online_1) = get_funded_noutxo_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    test_create_utxos(
        &mut wallet_1,
        online_1.clone(),
        false,
        Some(1),
        None,
        FEE_RATE,
    );

    // issue
    let asset_nia = wallet_1
        .issue_asset_nia(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_nia_cid = ContractId::from_str(&asset_nia.asset_id).unwrap();
    let asset_cfa = wallet_1
        .issue_asset_cfa(
            online_1.clone(),
            s!("NAME2"),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            Some(file_str.to_string()),
        )
        .unwrap();
    let asset_cfa_cid = ContractId::from_str(&asset_cfa.asset_id).unwrap();

    // check both assets are allocated to the same UTXO
    let unspents = wallet_1.list_unspents(None, true).unwrap();
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
    show_unspent_colorings(&wallet_1, "wallet 1 after issuance");

    //
    // 1st transfer, asset_nia: wallet 1 > wallet 2
    //

    // send
    println!("\n=== send 1");
    test_create_utxos(
        &mut wallet_1,
        online_1.clone(),
        false,
        Some(1),
        None,
        FEE_RATE,
    );
    let receive_data_1 = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet_1, &online_1, recipient_map);
    assert!(!txid_1.is_empty());
    show_unspent_colorings(&wallet_1, "wallet 1 after send 1, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_nia.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_nia.asset_id.clone()), vec![])
        .unwrap();

    // transfer 1 checks
    let transfers_w1 = wallet_1.list_transfers(asset_nia.asset_id.clone()).unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = wallet_2.list_transfers(asset_nia.asset_id.clone()).unwrap();
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
    let unspents = wallet_1.list_unspents(None, true).unwrap();
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
    let state_map_nia_w1 = wallet_1
        ._rgb_runtime()
        .unwrap()
        .state_for_outpoints(asset_nia_cid, change_outpoint_set.clone())
        .unwrap();
    check_state_map_asset_amount(&state_map_nia_w1, ca_a1.amount);
    let state_map_cfa_w1 = wallet_1
        ._rgb_runtime()
        .unwrap()
        .state_for_outpoints(asset_cfa_cid, change_outpoint_set.clone())
        .unwrap();
    check_state_map_asset_amount(&state_map_cfa_w1, ca_a2.amount);

    //
    // 2nd transfer, asset_cfa (blank in 1st send): wallet 1 > wallet 2
    //

    // send
    let receive_data_2 = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    println!("\n=== send 2");
    let recipient_map = HashMap::from([(
        asset_cfa.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send_default(&mut wallet_1, &online_1, recipient_map);
    assert!(!txid_2.is_empty());
    show_unspent_colorings(&wallet_1, "wallet 1 after send 2, WaitingCounterparty");

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_cfa.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    wallet_2.refresh(online_2, None, vec![]).unwrap();
    wallet_1
        .refresh(online_1, Some(asset_cfa.asset_id.clone()), vec![])
        .unwrap();

    // transfer 2 checks
    let transfers_w2 = wallet_2.list_transfers(asset_cfa.asset_id.clone()).unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = wallet_1.list_transfers(asset_cfa.asset_id.clone()).unwrap();
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
    let unspents = wallet_1.list_unspents(None, true).unwrap();
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
    let state_map_nia_w1 = wallet_1
        ._rgb_runtime()
        .unwrap()
        .state_for_outpoints(asset_nia_cid, change_outpoint_set.clone())
        .unwrap();
    check_state_map_asset_amount(&state_map_nia_w1, ca_a1.amount);
    let state_map_cfa_w1 = wallet_1
        ._rgb_runtime()
        .unwrap()
        .state_for_outpoints(asset_cfa_cid, change_outpoint_set.clone())
        .unwrap();
    check_state_map_asset_amount(&state_map_cfa_w1, ca_a2.amount);
}

#[test]
fn send_received_success() {
    initialize();

    let amount_1a: u64 = 66;
    let amount_1b: u64 = 33;
    let amount_2a: u64 = 7;
    let amount_2b: u64 = 4;
    let file_str = "README.md";

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!(true, true);
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // issue
    let asset_nia = wallet_1
        .issue_asset_nia(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_cfa = wallet_1
        .issue_asset_cfa(
            online_1.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            Some(file_str.to_string()),
        )
        .unwrap();

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_a20 = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_a25 = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_a20.recipient_id).unwrap(),
                ),
                amount: amount_1a,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_a25.recipient_id).unwrap(),
                ),
                amount: amount_1b,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_1 = test_send_default(&mut wallet_1, &online_1, recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1.refresh(online_1.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1.refresh(online_1, None, vec![]).unwrap();

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

    let unspents = wallet_1.list_unspents(None, true).unwrap();
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
    let receive_data_b20 = wallet_3
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_b25 = wallet_3
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([
        (
            asset_nia.asset_id.clone(),
            vec![Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_b20.recipient_id).unwrap(),
                ),
                amount: amount_2a,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_cfa.asset_id.clone(),
            vec![Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_b25.recipient_id).unwrap(),
                ),
                amount: amount_2b,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid_2 = test_send_default(&mut wallet_2, &online_2, recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_3.refresh(online_3.clone(), None, vec![]).unwrap();
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    mine(false);
    wallet_3.refresh(online_3, None, vec![]).unwrap();
    wallet_2.refresh(online_2, None, vec![]).unwrap();

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

    let unspents = wallet_2.list_unspents(None, true).unwrap();
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

    // check RGB25 asset has the correct attachment after being received
    let cfa_assets = wallet_3
        .list_assets(vec![AssetIface::RGB25])
        .unwrap()
        .cfa
        .unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let recv_asset = cfa_assets.first().unwrap();
    let dst_path = recv_asset.data_paths.first().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = hex::encode(src_hash.to_byte_array());
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);
}

#[test]
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
    let asset = wallet_1
        .issue_asset_cfa(
            online_1.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            Some(file_str.to_string()),
        )
        .unwrap();

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let receive_data_1 = wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            amount: amount_1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet_1, &online_1, recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    wallet_2.refresh(online_2.clone(), None, vec![]).unwrap();
    wallet_1
        .refresh(online_1, Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // transfer 1 checks
    let (transfer_w1, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let transfer_w2 = get_test_transfer_recipient(&wallet_2, &receive_data_1.recipient_id);
    let (transfer_data_w1, _) = get_test_transfer_data(&wallet_1, &transfer_w1);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w1.amount, amount_1.to_string());
    assert_eq!(transfer_w2.amount, amount_1.to_string());
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = wallet_1.list_unspents(None, true).unwrap();
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
    let receive_data_2 = wallet_3
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send_default(&mut wallet_2, &online_2, recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_3.refresh(online_3.clone(), None, vec![]).unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    wallet_3.refresh(online_3, None, vec![]).unwrap();
    wallet_2
        .refresh(online_2, Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // transfer 2 checks
    let transfer_w3 = get_test_transfer_recipient(&wallet_3, &receive_data_2.recipient_id);
    let (transfer_w2, _, _) = get_test_transfer_sender(&wallet_2, &txid_2);
    let (transfer_data_w3, _) = get_test_transfer_data(&wallet_3, &transfer_w3);
    let (transfer_data_w2, _) = get_test_transfer_data(&wallet_2, &transfer_w2);
    assert_eq!(transfer_w3.amount, amount_2.to_string());
    assert_eq!(transfer_w2.amount, amount_2.to_string());
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = wallet_2.list_unspents(None, true).unwrap();
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
        .list_assets(vec![AssetIface::RGB25])
        .unwrap()
        .cfa
        .unwrap();
    assert_eq!(cfa_assets.len(), 1);
    let recv_asset = cfa_assets.first().unwrap();
    assert_eq!(recv_asset.asset_id, asset.asset_id);
    assert_eq!(recv_asset.name, NAME.to_string());
    assert_eq!(recv_asset.description, Some(DESCRIPTION.to_string()));
    assert_eq!(recv_asset.precision, PRECISION);
    assert_eq!(
        recv_asset.balance,
        Balance {
            settled: amount_2,
            future: amount_2,
            spendable: amount_2,
        }
    );
    // check attachment mime-type
    let media = recv_asset.data_paths.first().unwrap();
    assert_eq!(media.mime, "text/plain");
    // check attachment data matches
    let dst_path = recv_asset.data_paths.first().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check attachment id for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = hex::encode(src_hash.to_byte_array());
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);
}

#[test]
fn receive_multiple_same_asset_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount: amount_1,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                amount: amount_2,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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

    stop_mining();

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

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
    mine(true);
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()), vec![])
        .unwrap();

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
    let unspents = wallet.list_unspents(None, true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_1.change_utxo);
    assert!(change_unspent.is_some());
}

#[test]
fn receive_multiple_different_assets_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset_1 = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_2 = wallet
        .issue_asset_cfa(
            online.clone(),
            s!("NAME2"),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
        )
        .unwrap();

    // send
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![Recipient {
                amount: amount_1,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_2.asset_id.clone(),
            vec![Recipient {
                amount: amount_2,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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

    stop_mining();

    // transfers progress to status WaitingConfirmations after a refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset_1.asset_id.clone()), vec![])
        .unwrap();

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
    let rcv_assets = rcv_wallet.list_assets(vec![]).unwrap();
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
    assert_eq!(rcv_asset_cfa.description, Some(DESCRIPTION.to_string()));
    assert_eq!(rcv_asset_cfa.precision, PRECISION);
    assert_eq!(
        rcv_asset_cfa.balance,
        Balance {
            settled: 0,
            future: amount_2,
            spendable: 0,
        }
    );
    let empty_data_paths = vec![];
    assert_eq!(rcv_asset_cfa.data_paths, empty_data_paths);

    // transfers progress to status Settled after tx mining + refresh
    std::thread::sleep(Duration::from_millis(1000)); // make sure updated_at will be at least +1s
    mine(true);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset_1.asset_id.clone()), vec![])
        .unwrap();

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
    let unspents = wallet.list_unspents(None, true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_1.change_utxo);
    assert!(change_unspent.is_some());
}

#[test]
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
    let asset_a = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_b = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let _asset_c = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    show_unspent_colorings(&wallet, "after issuances");

    // check each assets is allocated to a different UTXO
    let unspents = wallet.list_unspents(None, true).unwrap();
    let unspents_with_rgb_allocations = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty());
    assert_eq!(unspents_with_rgb_allocations.count(), 3);

    // blind
    let receive_data_a1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_a2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_b1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_b2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // send multiple assets to multiple recipients
    let recipient_map = HashMap::from([
        (
            asset_a.asset_id.clone(),
            vec![
                Recipient {
                    recipient_data: RecipientData::BlindedUTXO(
                        SecretSeal::from_str(&receive_data_a1.recipient_id).unwrap(),
                    ),
                    amount: amount_a1,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    recipient_data: RecipientData::BlindedUTXO(
                        SecretSeal::from_str(&receive_data_a2.recipient_id).unwrap(),
                    ),
                    amount: amount_a2,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
        (
            asset_b.asset_id.clone(),
            vec![
                Recipient {
                    recipient_data: RecipientData::BlindedUTXO(
                        SecretSeal::from_str(&receive_data_b1.recipient_id).unwrap(),
                    ),
                    amount: amount_b1,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
                Recipient {
                    recipient_data: RecipientData::BlindedUTXO(
                        SecretSeal::from_str(&receive_data_b2.recipient_id).unwrap(),
                    ),
                    amount: amount_b2,
                    transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
                },
            ],
        ),
    ]);
    let txid = wallet
        .send(online, recipient_map, true, FEE_RATE, MIN_CONFIRMATIONS)
        .unwrap();
    assert!(!txid.is_empty());

    show_unspent_colorings(&wallet, "after send");

    // check change UTXO has all the expected allocations
    let transfers_a = wallet.list_transfers(asset_a.asset_id.clone()).unwrap();
    let transfer_a = transfers_a.last().unwrap();
    let change_utxo = transfer_a.change_utxo.as_ref().unwrap();
    let unspents = wallet.list_unspents(None, false).unwrap();
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
    rcv_wallet_1
        .refresh(rcv_online_1.clone(), None, vec![])
        .unwrap();
    rcv_wallet_2
        .refresh(rcv_online_2.clone(), None, vec![])
        .unwrap();
    rcv_wallet_1
        .list_transfers(asset_a.asset_id.clone())
        .unwrap();
    rcv_wallet_1
        .list_transfers(asset_b.asset_id.clone())
        .unwrap();
    rcv_wallet_2
        .list_transfers(asset_a.asset_id.clone())
        .unwrap();
    rcv_wallet_2
        .list_transfers(asset_b.asset_id.clone())
        .unwrap();
    mine(false);
    rcv_wallet_1.refresh(rcv_online_1, None, vec![]).unwrap();
    rcv_wallet_2.refresh(rcv_online_2, None, vec![]).unwrap();
    rcv_wallet_1
        .list_transfers(asset_a.asset_id.clone())
        .unwrap();
    rcv_wallet_1
        .list_transfers(asset_b.asset_id.clone())
        .unwrap();
    rcv_wallet_2
        .list_transfers(asset_a.asset_id.clone())
        .unwrap();
    rcv_wallet_2
        .list_transfers(asset_b.asset_id.clone())
        .unwrap();

    show_unspent_colorings(&wallet, "after send, settled");
}

#[test]
fn reuse_failed_blinded_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map.clone());
    assert!(!txid.is_empty());

    // try to send again and check the asset is not spendable
    let result = wallet.send(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: id }) if id == asset.asset_id)
    );

    // fail transfer so asset allocation can be spent again
    wallet
        .fail_transfers(online.clone(), None, Some(txid), false)
        .unwrap();

    // 2nd transfer using the same blinded UTXO
    let result = wallet.send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(matches!(result, Err(Error::BlindedUTXOAlreadyUsed)));
}

#[test]
fn ack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, rcv_online_2) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with donation set to false
    let receive_data_1 = rcv_wallet_1
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_2 = rcv_wallet_2
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
            Recipient {
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
                ),
                amount,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            },
        ],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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
    rcv_wallet_1.refresh(rcv_online_1, None, vec![]).unwrap();
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
    rcv_wallet_2.refresh(rcv_online_2, None, vec![]).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &receive_data_2.recipient_id,
        TransferStatus::WaitingConfirmations
    ));

    // now sender can broadcast and move on to WaitingConfirmations
    wallet
        .refresh(online, Some(asset.asset_id), vec![])
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingConfirmations
    ));
}

#[test]
fn nack() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with donation set to false
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
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
    wallet
        .refresh(online, Some(asset.asset_id), vec![])
        .unwrap();
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::Failed
    ));
}

#[test]
fn expire() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());

    // check expiration is set correctly
    let (transfer, _, batch_transfer) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(
        transfer_data.expiration,
        Some(transfer_data.created_at + DURATION_SEND_TRANSFER)
    );

    // manually set expiration time in the near future to speed up the test
    let mut updated_transfer: DbBatchTransferActMod = batch_transfer.into();
    updated_transfer.expiration = ActiveValue::Set(Some(transfer_data.created_at + 1));
    wallet
        .database
        .update_batch_transfer(&mut updated_transfer)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2000));
    //
    // expire transfer + check status goes to Failed
    let mut db_data = wallet.database.get_db_data(false).unwrap();
    wallet._handle_expired_transfers(&mut db_data).unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
fn no_change_on_pending_send() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 32;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), true, Some(3), None, FEE_RATE);
    assert_eq!(num_utxos_created, 3);

    // issue 1 + get its UTXO
    let asset_1 = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspent_1 = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        })
        .unwrap();
    // issue 2
    let asset_2 = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT * 2],
        )
        .unwrap();

    show_unspent_colorings(&wallet, "before 1st send");
    // send asset_1
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_1.is_empty());

    // send asset_2 (send_1 in WaitingCounterparty)
    show_unspent_colorings(&wallet, "before 2nd send");
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_2.is_empty());
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));
    // fail send asset_2
    wallet
        .fail_transfers(online.clone(), None, Some(txid_2), false)
        .unwrap();

    stop_mining();

    // progress send_1 to WaitingConfirmations
    show_unspent_colorings(&wallet, "before refresh");
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online.clone(), Some(asset_1.asset_id.clone()), vec![])
        .unwrap();

    // send asset_2 (send_1 in WaitingConfirmations)
    show_unspent_colorings(&wallet, "before 3rd send");
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_2.asset_id,
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_3.is_empty());
    show_unspent_colorings(&wallet, "after 3rd send");
    // check change was not allocated on issue 1 UTXO (pending Input coloring)
    assert!(!unspent_1.rgb_allocations.iter().any(|a| !a.settled));

    resume_mining();
}

#[test]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    let mut wallet_1_alloc = get_test_wallet(true, Some(1));
    let online_1_alloc = wallet_1_alloc
        .go_online(true, ELECTRUM_URL.to_string())
        .unwrap();

    // cannot send if no available allocations for change (max 1 allocation per UTXO)
    fund_wallet(wallet_1_alloc.get_address());
    mine(false);
    test_create_utxos(
        &mut wallet_1_alloc,
        online_1_alloc.clone(),
        true,
        Some(1),
        None,
        FEE_RATE,
    );
    let asset_1_alloc = wallet_1_alloc
        .issue_asset_nia(
            online_1_alloc.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
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
        asset_1_alloc.asset_id,
        vec![Recipient {
            amount: AMOUNT / 2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet_1_alloc.send(
        online_1_alloc,
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // issue asset
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // insufficient assets (amount too big)
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT + 1,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(
        matches!(result, Err(Error::InsufficientTotalAssets { asset_id: t }) if t == asset.asset_id)
    );

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown:{PROXY_HOST}")];
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints,
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    let msg = s!("library supports at max 3 transport endpoints");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // fee min/max
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: AMOUNT / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        0.9,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = wallet.send_begin(online, recipient_map, false, 1000.1, MIN_CONFIRMATIONS);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));
}

#[test]
fn pending_incoming_transfer_fail() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_noutxo_wallet!();
    test_create_utxos(
        &mut rcv_wallet,
        rcv_online.clone(),
        false,
        Some(1),
        None,
        FEE_RATE,
    );

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    //
    // 1st transfer
    //

    // send
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    show_unspent_colorings(&wallet, "sender after 1st send, settled");
    show_unspent_colorings(&rcv_wallet, "receiver after 1st send, settled");

    //
    // 2nd transfer
    //

    // add a blind to the same UTXO
    let _receive_data_2 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    show_unspent_colorings(&rcv_wallet, "receiver after 2nd blind");

    // send from receiving wallet, 1st receive Settled, 2nd one still pending
    let receive_data = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: amount_2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    show_unspent_colorings(&wallet, "sender after 2nd send, WaitingCounterparty");
    show_unspent_colorings(&rcv_wallet, "receiver after 2nd send, WaitingCounterparty");
    // check input allocation is blocked by pending receive
    let result = rcv_wallet.send(
        rcv_online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()), vec![])
        .unwrap();
    // check input allocation is still blocked by pending receive
    let result = rcv_wallet.send(
        rcv_online,
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[test]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // 1st send
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());

    // 2nd send (1st still pending)
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount: amount / 2,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    // check input allocation is blocked by pending send
    let result = wallet.send(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    // check input allocation is still blocked by pending send
    let result = wallet.send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[test]
fn pending_transfer_input_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();
    test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // blind with sender wallet to create a pending transfer
    wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    show_unspent_colorings(&wallet, "sender after blind");

    // send and check it fails as the issuance UTXO is "blocked" by the pending receive operation
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(
        matches!(result, Err(Error::InsufficientSpendableAssets { asset_id: t }) if t == asset.asset_id)
    );
}

#[test]
fn already_used_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset to 3 UTXOs
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT * 2, AMOUNT * 3],
        )
        .unwrap();

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
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map.clone());
    assert!(!txid.is_empty());

    // 2nd transfer using the same blinded UTXO
    let result = wallet.send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(matches!(result, Err(Error::BlindedUTXOAlreadyUsed)));
}

#[test]
fn cfa_blank_success() {
    initialize();

    let amount_issue_ft = 10000;
    let amount_issue_nft = 1;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue RGB20
    let asset_nia = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT"),
            s!("Test Fungible Token"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // issue RGB25
    let _asset_cfa = wallet
        .issue_asset_cfa(
            online.clone(),
            s!("Test Non Funguble Token"),
            Some(s!("Debugging rgb blank error")),
            PRECISION,
            vec![amount_issue_nft],
            Some(s!("README.md")),
        )
        .unwrap();

    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // try sending RGB20
    let recipient_map = HashMap::from([(
        asset_nia.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(!res.unwrap().is_empty());
}

#[test]
fn psbt_rgb_consumer_success() {
    initialize();

    let amount_issue_ft = 10000;

    // create wallet with funds and no UTXOs
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO
    println!("utxo 1");
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), true, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // issue an RGB20 asset
    println!("issue 1");
    let asset_nia_a = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT1"),
            s!("Test Fungible Token 1"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // try to send it
    println!("send_begin 1");
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(!res.unwrap().is_empty());

    // issue one more RGB20 asset, should go to the same UTXO as the 1st issuance
    println!("issue 2");
    let asset_nia_b = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT2"),
            s!("Test Fungible Token 2"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // try to send the second asset
    println!("send_begin 2");
    let receive_data_2 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia_b.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(
        online.clone(),
        recipient_map,
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(!res.unwrap().is_empty());

    // exhaust allocations + issue 3rd asset, on a different UTXO
    println!("exhaust allocations on current UTXO");
    let new_allocation_count = (MAX_ALLOCATIONS_PER_UTXO - 2).max(0);
    for _ in 0..new_allocation_count {
        let _receive_data = wallet
            .blind_receive(
                None,
                None,
                None,
                TRANSPORT_ENDPOINTS.clone(),
                MIN_CONFIRMATIONS,
            )
            .unwrap();
    }
    println!("issue 3");
    let asset_nia_c = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT3"),
            s!("Test Fungible Token 3"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();
    // fail transfers so 1st UTXO can be used as input
    wallet
        .fail_transfers(online.clone(), None, None, false)
        .unwrap();

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 3");
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // try to send the second asset to a recipient and the third to different one
    println!("send_begin 3");
    let receive_data_3a = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let receive_data_3b = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([
        (
            asset_nia_b.asset_id,
            vec![Recipient {
                amount: 1,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_3a.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
        (
            asset_nia_c.asset_id,
            vec![Recipient {
                amount: 1,
                recipient_data: RecipientData::BlindedUTXO(
                    SecretSeal::from_str(&receive_data_3b.recipient_id).unwrap(),
                ),
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        ),
    ]);
    let res = wallet.send_begin(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(!res.unwrap().is_empty());
}

#[test]
fn insufficient_bitcoins() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send and drain the rest
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        online.clone(),
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    wallet
        .drain_to(online.clone(), rcv_wallet.get_address(), false, FEE_RATE)
        .unwrap();

    // issue an RGB20 asset
    let asset_nia_a = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT1"),
            s!("Test Fungible Token 1"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with no colorable UTXOs available as additional bitcoin inputs and no other funds
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 1);
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(
        res,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // create 1 UTXO for change (add funds, create UTXO, drain the rest)
    fund_wallet(wallet.get_address());
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        online.clone(),
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);
    wallet
        .drain_to(online.clone(), rcv_wallet.get_address(), false, FEE_RATE)
        .unwrap();

    // send works with no colorable UTXOs available as additional bitcoin inputs
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 2);
    let txid = wallet
        .send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS)
        .unwrap();
    assert!(!txid.is_empty());
}

#[test]
fn insufficient_allocations_fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        online.clone(),
        false,
        Some(1),
        Some(TINY_BTC_AMOUNT),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);

    // issue an RGB20 asset
    let asset_nia_a = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT1"),
            s!("Test Fungible Token 1"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with no colorable UTXOs available as change
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 2);
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(res, Err(Error::InsufficientAllocationSlots)));

    // create 1 more UTXO for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), false, Some(1), None, FEE_RATE);
    assert_eq!(num_utxos_created, 1);

    // send works with no colorable UTXOs available as additional bitcoin inputs
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 3);
    let txid = wallet
        .send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS)
        .unwrap();
    assert!(!txid.is_empty());
}

#[test]
fn insufficient_allocations_success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 UTXO with not enough bitcoins for a send
    let num_utxos_created = test_create_utxos(
        &mut wallet,
        online.clone(),
        false,
        Some(1),
        Some(300),
        FEE_RATE,
    );
    assert_eq!(num_utxos_created, 1);

    // issue an RGB20 asset on the unspendable UTXO
    let asset_nia_a = wallet
        .issue_asset_nia(
            online.clone(),
            s!("TFT1"),
            s!("Test Fungible Token 1"),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // create 2 more UTXOs, 1 for change + 1 as additional bitcoin input
    let num_utxos_created =
        test_create_utxos(&mut wallet, online.clone(), false, Some(2), None, FEE_RATE);
    assert_eq!(num_utxos_created, 2);

    // send with 1 colorable UTXOs available as additional bitcoin input
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia_a.asset_id,
        vec![Recipient {
            amount: 1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let res = wallet.send_begin(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(!res.unwrap().is_empty());
}

#[test]
fn send_to_oneself() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let receive_data = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let result = wallet.send(online, recipient_map, false, FEE_RATE, MIN_CONFIRMATIONS);
    assert!(matches!(result, Err(Error::CannotSendToSelf)));
}

#[test]
fn send_received_back_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    //
    // 1st transfer: from issuer to recipient
    //

    // send
    let receive_data_1 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // transfer 1 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_1.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_1.to_string());
    assert_eq!(transfer.amount, amount_1.to_string());

    let unspents = wallet.list_unspents(None, true).unwrap();
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
    let receive_data_2 = wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send_default(&mut rcv_wallet, &rcv_online, recipient_map);
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    rcv_wallet
        .refresh(rcv_online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    wallet.refresh(online.clone(), None, vec![]).unwrap();
    rcv_wallet
        .refresh(rcv_online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();

    // transfer 2 checks
    let rcv_transfer = get_test_transfer_recipient(&wallet, &receive_data_2.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&rcv_wallet, &txid_2);
    let (transfer_data, _) = get_test_transfer_data(&rcv_wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_2.to_string());
    assert_eq!(transfer.amount, amount_2.to_string());

    let unspents = rcv_wallet.list_unspents(None, true).unwrap();
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

    show_unspent_colorings(&wallet, "wallet before 3rd transfer");
    show_unspent_colorings(&rcv_wallet, "rcv_wallet before 3rd transfer");
    // send
    let receive_data_3 = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let change_3 = 5;
    let amount_3 = wallet
        .get_asset_balance(asset.asset_id.clone())
        .unwrap()
        .spendable
        - change_3;
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_3, // make sure to spend received transfer allocation
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_3.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid_3.is_empty());
    show_unspent_colorings(&wallet, "wallet after 3rd transfer");
    show_unspent_colorings(&rcv_wallet, "rcv_wallet after 3rd transfer");

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    mine(false);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id), vec![])
        .unwrap();

    // transfer 3 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data_3.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_3);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.amount, amount_3.to_string());
    assert_eq!(transfer.amount, amount_3.to_string());

    let unspents = wallet.list_unspents(None, true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(change_allocations.first().unwrap().amount, change_3);
}

#[test]
fn witness_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let receive_data = rcv_wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::WitnessData {
                script_buf: ScriptBuf::from_hex(&receive_data.recipient_id).unwrap(),
                amount_sat: 1000,
                blinding: None,
            },
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());

    stop_mining();

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, rcv_asset_transfer) =
        get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()), vec![])
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);

    assert_eq!(rcv_transfer_data.kind, TransferKind::ReceiveWitness);
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingConfirmations
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingConfirmations);
    assert_eq!(rcv_transfer.amount, amount.to_string());
    // ack is now true on the sender side
    assert_eq!(transfer.ack, Some(true));
    // asset id is now set on the receiver side
    assert_eq!(rcv_asset_transfer.asset_id, Some(asset.asset_id.clone()));

    // asset has been received correctly
    let rcv_assets = rcv_wallet.list_assets(vec![]).unwrap();
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
            future: 0, // still 0 for witness transfers
            spendable: 0,
        }
    );

    // transfers progress to status Settled after tx mining + refresh
    mine(true);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id), vec![])
        .unwrap();

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &receive_data.recipient_id);
    let (rcv_transfer_data, _) = get_test_transfer_data(&rcv_wallet, &rcv_transfer);
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    // change is unspent once transfer is Settled
    let unspents = wallet.list_unspents(None, true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo);
    assert!(change_unspent.is_some());
}
