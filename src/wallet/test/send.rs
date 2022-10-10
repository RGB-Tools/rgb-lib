use super::*;
use rgb::{OutpointState, StateAtom};

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo.clone(),
        }],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &blind_data.blinded_utxo);
    let rcv_transfer_data = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer)
        .unwrap();
    let rcv_asset_transfer = get_test_asset_transfer(&rcv_wallet, rcv_transfer.asset_transfer_idx);
    let (transfer, asset_transfer, _) = get_test_transfer_sender(&wallet, &txid);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();

    // ack is None
    assert_eq!(rcv_transfer.ack, None);
    assert_eq!(transfer.ack, None);
    // amount is set only for the sender
    assert_eq!(rcv_transfer.amount, 0.to_string());
    assert_eq!(transfer.amount, amount.to_string());
    // blinded_utxo is set
    assert_eq!(
        rcv_transfer.blinded_utxo,
        Some(blind_data.blinded_utxo.clone())
    );
    assert_eq!(transfer.blinded_utxo, Some(blind_data.blinded_utxo.clone()));
    // blindind_secret
    assert!(rcv_transfer.blinding_secret.is_some());
    assert!(transfer.blinding_secret.is_none());

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
    assert!(rcv_transfer_data.incoming);
    assert!(!transfer_data.incoming);
    // transfers start in WaitingCounterparty status
    assert_eq!(
        rcv_transfer_data.status,
        TransferStatus::WaitingCounterparty
    );
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);
    // txid is set only for the sender
    assert_eq!(rcv_transfer_data.txid, None);
    assert_eq!(transfer_data.txid, Some(txid.clone()));
    // unblinded UTXO is set only for the receiver
    assert!(rcv_transfer_data.unblinded_utxo.is_some());
    assert!(transfer_data.unblinded_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer.asset_id.is_none());
    assert_eq!(asset_transfer.asset_id, Some(asset.asset_id.clone()));
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer.user_driven);
    assert!(asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &blind_data.blinded_utxo);
    let rcv_transfer_data = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer)
        .unwrap();
    let rcv_asset_transfer = get_test_asset_transfer(&rcv_wallet, rcv_transfer.asset_transfer_idx);
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
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

    // transfers progress to status Settled after tx mining + refresh
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet.refresh(online, Some(asset.asset_id)).unwrap();

    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &blind_data.blinded_utxo);
    let rcv_transfer_data = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer)
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    // update timestamp has been updated
    assert!(rcv_transfer_data.updated_at > rcv_updated_at);
    assert!(transfer_data.updated_at > updated_at);

    // change is unspent once transfer is Settled
    wallet._sync_db_txos().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data.change_utxo);
    assert!(change_unspent.is_some());
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
        .issue_asset(
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
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            blinded_utxo: blind_data_1.blinded_utxo.clone(),
        }],
    )]);
    let txid_1 = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();

    // transfer 1 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_data = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer)
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_1);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
    assert_eq!(rcv_transfer.amount, amount_1.to_string());
    assert_eq!(transfer.amount, amount_1.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = wallet.list_unspents(true).unwrap();
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
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            blinded_utxo: blind_data_2.blinded_utxo.clone(),
        }],
    )]);
    let txid_2 = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet.refresh(online, Some(asset.asset_id)).unwrap();

    // transfer 2 checks
    let rcv_transfer = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer)
        .unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid_2);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
    assert_eq!(rcv_transfer.amount, amount_2.to_string());
    assert_eq!(transfer.amount, amount_2.to_string());
    assert_eq!(rcv_transfer_data.status, TransferStatus::Settled);
    assert_eq!(transfer_data.status, TransferStatus::Settled);

    let unspents = wallet.list_unspents(true).unwrap();
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

    fn check_state_map_asset_amount(
        state_map: &BTreeMap<ContractId, BTreeMap<OutPoint, BTreeSet<OutpointState>>>,
        asset_id: &String,
        outpoint: &OutPoint,
        amount: u64,
    ) {
        let asset_state_map = state_map
            .iter()
            .find(|e| &e.0.to_string() == asset_id)
            .unwrap();
        let outpoint_state_set = asset_state_map
            .1
            .iter()
            .find(|e| e.0 == outpoint)
            .unwrap()
            .1;
        let outpoint_state = outpoint_state_set.iter().find(|_e| true).unwrap();
        if let StateAtom::Value(revealed) = outpoint_state.state.clone() {
            assert_eq!(revealed.value, amount);
        } else {
            panic!("unexpected");
        };
    }

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;

    // wallets
    println!("wallet 1");
    let (mut wallet_1, online_1) = get_funded_wallet!(true, true);
    println!("wallet 2");
    let (mut wallet_2, online_2) = get_funded_wallet!(true, true);

    // issue
    println!("asset 1");
    let asset_1 = wallet_1
        .issue_asset(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    println!("asset 2");
    let asset_2 = wallet_1
        .issue_asset(
            online_1.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION,
            vec![AMOUNT * 2],
        )
        .unwrap();

    // check both assets are allocated to the same utxo
    wallet_1._sync_db_txos().unwrap();
    let unspents = wallet_1.list_unspents(true).unwrap();
    let unspents_with_rgb_allocations: Vec<Unspent> = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert!(unspents_with_rgb_allocations.len() == 1);
    let allocation_asset_ids: Vec<String> = unspents_with_rgb_allocations
        .first()
        .unwrap()
        .rgb_allocations
        .clone()
        .into_iter()
        .map(|a| a.asset_id.unwrap_or_else(|| s!("")))
        .collect();
    assert!(allocation_asset_ids.contains(&asset_1.asset_id));
    assert!(allocation_asset_ids.contains(&asset_2.asset_id));

    //
    // 1st transfer, asset_1: wallet 1 > wallet 2
    //

    // send
    println!("\n=== send 1");
    let blind_data_1 = wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data_1.blinded_utxo,
            amount: amount_1,
        }],
    )]);
    let txid_1 = wallet_1
        .send(online_1.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    println!("refresh");
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_1.asset_id.clone()))
        .unwrap();
    mine();
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_1.asset_id.clone()))
        .unwrap();

    // transfer 1 checks
    println!("check");
    let transfers_w1 = wallet_1.list_transfers(asset_1.asset_id.clone()).unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = wallet_2.list_transfers(asset_1.asset_id.clone()).unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.amount, amount_1);
    assert!(!transfer_w1.incoming);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.amount, amount_1);
    assert!(transfer_w2.incoming);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = wallet_1.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let ca_a1 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_2.asset_id.clone()))
        .unwrap();
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_1.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2);
    assert_eq!(ca_a2.asset_id, Some(asset_2.asset_id.clone()));
    assert!(ca_a2.settled);
    // sender RGB state map
    let mut change_outpoint_set = BTreeSet::new();
    change_outpoint_set.insert(OutPoint::from(change_utxo.clone()));
    let state_map_w1 = wallet_1
        ._rgb_client()
        .unwrap()
        .outpoint_state(change_outpoint_set.clone(), |_| ())
        .unwrap();
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_1.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint.clone()),
        ca_a1.amount,
    );
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_2.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint),
        ca_a2.amount,
    );

    //
    // 2nd transfer, asset_2 (blank in 1st send): wallet 1 > wallet 2
    //

    // send
    let blind_data_2 = wallet_2.blind(None, None).unwrap();
    println!("\n=== send 2");
    let recipient_map = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data_2.blinded_utxo,
            amount: amount_2,
        }],
    )]);
    let txid_2 = wallet_1
        .send(online_1.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    println!("refresh");
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_2.asset_id.clone()))
        .unwrap();
    mine();
    wallet_2.refresh(online_2, None).unwrap();
    wallet_1
        .refresh(online_1, Some(asset_2.asset_id.clone()))
        .unwrap();

    // transfer 2 checks
    println!("check");
    let transfers_w2 = wallet_2.list_transfers(asset_2.asset_id.clone()).unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = wallet_1.list_transfers(asset_2.asset_id.clone()).unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    // transfers data
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.amount, amount_2);
    assert!(!transfer_w1.incoming);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.amount, amount_2);
    assert!(transfer_w2.incoming);
    // sender change
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = wallet_1.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 2);
    let ca_a1 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_1.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_2.asset_id.clone()))
        .unwrap();
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_1.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2 - amount_2);
    assert_eq!(ca_a2.asset_id, Some(asset_2.asset_id.clone()));
    assert!(ca_a2.settled);
    // sender RGB state map
    let mut change_outpoint_set = BTreeSet::new();
    change_outpoint_set.insert(OutPoint::from(change_utxo.clone()));
    let state_map_w1 = wallet_1
        ._rgb_client()
        .unwrap()
        .outpoint_state(change_outpoint_set.clone(), |_| ())
        .unwrap();
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_1.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint.clone()),
        ca_a1.amount,
    );
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_2.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint),
        ca_a2.amount,
    );
}

#[test]
fn send_received_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!(true, true);

    // issue
    let asset = wallet_1
        .issue_asset(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let blind_data_1 = wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data_1.blinded_utxo.clone(),
            amount: amount_1,
        }],
    )]);
    let txid_1 = wallet_1
        .send(online_1.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset.asset_id.clone()))
        .unwrap();

    // transfer 1 checks
    let (transfer_w1, _, _) = get_test_transfer_sender(&wallet_1, &txid_1);
    let transfer_w2 = get_test_transfer_recipient(&wallet_2, &blind_data_1.blinded_utxo);
    let transfer_data_w1 = wallet_1.database.get_transfer_data(&transfer_w1).unwrap();
    let transfer_data_w2 = wallet_2.database.get_transfer_data(&transfer_w2).unwrap();
    assert_eq!(transfer_w1.amount, amount_1.to_string());
    assert_eq!(transfer_w2.amount, amount_1.to_string());
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = wallet_1.list_unspents(true).unwrap();
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
    // 2nd transfer: wallet 2 > wallet 1
    //

    // send
    let blind_data_2 = wallet_1.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data_2.blinded_utxo.clone(),
            amount: amount_2,
        }],
    )]);
    let txid_2 = wallet_2
        .send(online_2.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_1.refresh(online_1.clone(), None).unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    wallet_1.refresh(online_1, None).unwrap();
    wallet_2.refresh(online_2, Some(asset.asset_id)).unwrap();

    // transfer 2 checks
    let transfer_w1 = get_test_transfer_recipient(&wallet_1, &blind_data_2.blinded_utxo);
    let (transfer_w2, _, _) = get_test_transfer_sender(&wallet_2, &txid_2);
    let transfer_data_w1 = wallet_1.database.get_transfer_data(&transfer_w1).unwrap();
    let transfer_data_w2 = wallet_2.database.get_transfer_data(&transfer_w2).unwrap();
    assert_eq!(transfer_w1.amount, amount_2.to_string());
    assert_eq!(transfer_w2.amount, amount_2.to_string());
    assert_eq!(transfer_data_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2.status, TransferStatus::Settled);

    let unspents = wallet_2.list_unspents(true).unwrap();
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
}

#[test]
fn batch_donation_success() {
    initialize();

    let amount_a1 = 11;
    let amount_a2 = 12;
    let amount_b1 = 21;
    let amount_b2 = 22;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet_1, rcv_online_1) = get_funded_wallet!();
    let (mut rcv_wallet_2, rcv_online_2) = get_funded_wallet!();

    // issue
    let asset_a = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_b = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_c = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // check all assets are allocated to the same utxo
    wallet._sync_db_txos().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let unspents_with_rgb_allocations: Vec<Unspent> = unspents
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    assert!(unspents_with_rgb_allocations.len() == 1);
    let allocation_asset_ids: Vec<String> = unspents_with_rgb_allocations
        .first()
        .unwrap()
        .rgb_allocations
        .clone()
        .into_iter()
        .map(|a| a.asset_id.unwrap_or_else(|| s!("")))
        .collect();
    assert!(allocation_asset_ids.contains(&asset_a.asset_id));
    assert!(allocation_asset_ids.contains(&asset_b.asset_id));
    assert!(allocation_asset_ids.contains(&asset_c.asset_id));

    // blind
    let blind_data_a1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_a2 = rcv_wallet_2.blind(None, None).unwrap();
    let blind_data_b1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_b2 = rcv_wallet_2.blind(None, None).unwrap();

    // send multiple assets to multiple recipients
    let recipient_map = HashMap::from([
        (
            asset_a.asset_id.clone(),
            vec![
                Recipient {
                    blinded_utxo: blind_data_a1.blinded_utxo,
                    amount: amount_a1,
                },
                Recipient {
                    blinded_utxo: blind_data_a2.blinded_utxo,
                    amount: amount_a2,
                },
            ],
        ),
        (
            asset_b.asset_id.clone(),
            vec![
                Recipient {
                    blinded_utxo: blind_data_b1.blinded_utxo,
                    amount: amount_b1,
                },
                Recipient {
                    blinded_utxo: blind_data_b2.blinded_utxo,
                    amount: amount_b2,
                },
            ],
        ),
    ]);
    let txid = wallet.send(online, recipient_map, true).unwrap();
    assert!(!txid.is_empty());

    // check change UTXO has all the expected allocations
    let transfers_a = wallet.list_transfers(asset_a.asset_id.clone()).unwrap();
    let transfer_a = transfers_a.last().unwrap();
    let change_utxo = transfer_a.change_utxo.as_ref().unwrap();
    let unspents = wallet.list_unspents(false).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 3);
    let allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_a.asset_id.clone()));
    let allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_b.asset_id.clone()));
    let allocation_c = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_c.asset_id.clone()));
    assert_eq!(allocation_a.unwrap().amount, AMOUNT - amount_a1 - amount_a2);
    assert_eq!(allocation_b.unwrap().amount, AMOUNT - amount_b1 - amount_b2);
    assert_eq!(allocation_c.unwrap().amount, AMOUNT);

    // take receiver transfers from WaitingCounterparty to Settled
    // (send_batch doesn't wait for recipient ACKs and proceeds to broadcast)
    rcv_wallet_1.refresh(rcv_online_1.clone(), None).unwrap();
    rcv_wallet_2.refresh(rcv_online_2.clone(), None).unwrap();
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
    mine();
    rcv_wallet_1.refresh(rcv_online_1, None).unwrap();
    rcv_wallet_2.refresh(rcv_online_2, None).unwrap();
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // 1st transfer
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    let txid = wallet
        .send(online.clone(), recipient_map.clone(), false)
        .unwrap();
    assert!(!txid.is_empty());

    // fail transfer so asset allocation can be spent again
    wallet
        .fail_transfers(online.clone(), None, Some(txid))
        .unwrap();

    // 2nd transfer using the same blinded utxo
    let txid = wallet.send(online, recipient_map, false).unwrap();
    assert!(!txid.is_empty());
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with donation set to false
    let blind_data_1 = rcv_wallet_1.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
                amount,
            },
            Recipient {
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
                amount,
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    // all transfers are in WaitingCounterparty status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &blind_data_1.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));

    // ack from recipient 1 > its transfer status changes to WaitingConfirmations
    rcv_wallet_1.refresh(rcv_online_1, None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_1,
        &blind_data_1.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));

    // ack from recipient 2 > its transfer status changes to WaitingConfirmations
    rcv_wallet_2.refresh(rcv_online_2, None).unwrap();
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet_2,
        &blind_data_2.blinded_utxo,
        TransferStatus::WaitingConfirmations
    ));

    // now sender can broadcast and move on to WaitingConfirmations
    wallet.refresh(online, Some(asset.asset_id)).unwrap();
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send with donation set to false
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo.clone(),
            amount,
        }],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    // transfers are in WaitingCounterparty status
    assert!(check_test_transfer_status_recipient(
        &rcv_wallet,
        &blind_data.blinded_utxo,
        TransferStatus::WaitingCounterparty
    ));
    assert!(check_test_transfer_status_sender(
        &wallet,
        &txid,
        TransferStatus::WaitingCounterparty
    ));

    // manually nack the transfer (consignment is valid so refreshing receiver would yield an ack)
    rcv_wallet
        .rest_client
        .post_nack(PROXY_URL, blind_data.blinded_utxo)
        .unwrap();

    // refreshing sender transfer now has it fail
    wallet.refresh(online, Some(asset.asset_id)).unwrap();
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    let txid = wallet.send(online, recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    // check expiration is set correctly
    let (transfer, _, batch_transfer) = get_test_transfer_sender(&wallet, &txid);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
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
    wallet._handle_expired_transfers().unwrap();
    let (transfer, _, _) = get_test_transfer_sender(&wallet, &txid);
    let transfer_data = wallet.database.get_transfer_data(&transfer).unwrap();
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    // blind
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();

    // invalid input (asset id)
    let recipient_map = HashMap::from([(
        s!("rgb1inexistent"),
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo.clone(),
            amount: AMOUNT / 2,
        }],
    )]);
    let result = wallet.send(online.clone(), recipient_map, false);
    assert!(matches!(result, Err(Error::AssetNotFound(_))));

    // invalid input (blinded utxo)
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: s!("invalid"),
            amount: AMOUNT / 2,
        }],
    )]);
    let result = wallet.send(online.clone(), recipient_map, false);
    assert!(matches!(result, Err(Error::InvalidBlindedUTXO(_))));

    // insufficient assets (amount too big)
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount: AMOUNT + 1,
        }],
    )]);
    let result = wallet.send(online, recipient_map, false);
    assert!(matches!(result, Err(Error::InsufficientAssets)));
}

#[test]
fn pending_incoming_transfer_fail() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 33;
    let amount_3: u64 = 7;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset(
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
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_1,
            blinded_utxo: blind_data_1.blinded_utxo,
        }],
    )]);
    let txid_1 = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();

    //
    // 2nd transfer
    //

    // send
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            amount: amount_2,
            blinded_utxo: blind_data_2.blinded_utxo,
        }],
    )]);
    let txid_2 = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid_2.is_empty());

    // send from receiving wallet, 1st receive Settled, 2nd one still pending
    let blind_data = wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount: amount_3,
        }],
    )]);
    let result = wallet.send(online, recipient_map, false);
    assert!(matches!(result, Err(Error::InsufficientAssets)));
}

#[test]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // 1st send
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount,
        }],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    // 2nd send (1st still pending)
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount: amount / 2,
        }],
    )]);
    let result = wallet.send(online, recipient_map, false);
    assert!(matches!(result, Err(_)));
}

#[test]
fn pending_transfer_input_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // blind with sender wallet to create a pending transfer
    wallet.blind(None, None).unwrap();

    // send anche check it fails as the issuance utxo is "blocked" by the pending receive operation
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            blinded_utxo: blind_data.blinded_utxo,
            amount,
        }],
    )]);
    let result = wallet.send(online, recipient_map, false);
    assert!(matches!(result, Err(Error::InsufficientAssets)));
}

#[test]
fn already_used_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue asset to 3 utxos
    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT * 2, AMOUNT * 3],
        )
        .unwrap();

    // 1st transfer
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id,
        vec![Recipient {
            amount,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    let txid = wallet
        .send(online.clone(), recipient_map.clone(), false)
        .unwrap();
    assert!(!txid.is_empty());

    // 2nd transfer using the same blinded utxo
    let result = wallet.send(online, recipient_map, false);
    assert!(matches!(result, Err(Error::BlindedUTXOAlreadyUsed)));
}
