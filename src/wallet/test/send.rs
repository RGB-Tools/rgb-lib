use rgb::{OutpointState, StateAtom};

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
        .issue_asset_rgb20(
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
    assert!(rcv_asset_transfer.asset_rgb20_id.is_none());
    assert_eq!(asset_transfer.asset_rgb20_id, Some(asset.asset_id.clone()));
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
    assert_eq!(
        rcv_asset_transfer.asset_rgb20_id,
        Some(asset.asset_id.clone())
    );
    // update timestamp has been updated
    let rcv_updated_at = rcv_transfer_data.updated_at;
    let updated_at = transfer_data.updated_at;
    assert!(rcv_updated_at > rcv_transfer_data.created_at);
    assert!(updated_at > transfer_data.created_at);

    // asset has been received correctly
    let rcv_assets = rcv_wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = rcv_assets.rgb20.unwrap();
    let rgb21_assets = rcv_assets.rgb21.unwrap();
    assert_eq!(rgb20_assets.len(), 1);
    assert_eq!(rgb21_assets.len(), 0);
    let rcv_asset = rgb20_assets.last().unwrap();
    assert_eq!(rcv_asset.asset_id, asset.asset_id);
    assert_eq!(rcv_asset.ticker, TICKER);
    assert_eq!(rcv_asset.name, NAME);
    assert_eq!(rcv_asset.precision, PRECISION);
    assert_eq!(
        rcv_asset.balance,
        Balance {
            settled: 0,
            future: amount
        }
    );

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
        .issue_asset_rgb20(
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
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
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
    let file_str = "README.md";

    // wallets
    println!("wallet 1");
    let (mut wallet_1, online_1) = get_funded_wallet!(true, true);
    println!("wallet 2");
    let (mut wallet_2, online_2) = get_funded_wallet!(true, true);

    // issue
    println!("asset 1");
    let asset_rgb20 = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    println!("asset 2");
    let asset_rgb21 = wallet_1
        .issue_asset_rgb21(
            online_1.clone(),
            s!("NAME2"),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
            Some(file_str.to_string()),
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
    assert!(allocation_asset_ids.contains(&asset_rgb20.asset_id));
    assert!(allocation_asset_ids.contains(&asset_rgb21.asset_id));

    //
    // 1st transfer, asset_rgb20: wallet 1 > wallet 2
    //

    // send
    println!("\n=== send 1");
    let blind_data_1 = wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_rgb20.asset_id.clone(),
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
        .refresh(online_1.clone(), Some(asset_rgb20.asset_id.clone()))
        .unwrap();
    mine();
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1
        .refresh(online_1.clone(), Some(asset_rgb20.asset_id.clone()))
        .unwrap();

    // transfer 1 checks
    println!("check");
    let transfers_w1 = wallet_1
        .list_transfers(asset_rgb20.asset_id.clone())
        .unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = wallet_2
        .list_transfers(asset_rgb20.asset_id.clone())
        .unwrap();
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
        .find(|a| a.asset_id == Some(asset_rgb20.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb21.asset_id.clone()))
        .unwrap();
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_rgb20.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2);
    assert_eq!(ca_a2.asset_id, Some(asset_rgb21.asset_id.clone()));
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
        &asset_rgb20.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint.clone()),
        ca_a1.amount,
    );
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_rgb21.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint),
        ca_a2.amount,
    );

    //
    // 2nd transfer, asset_rgb21 (blank in 1st send): wallet 1 > wallet 2
    //

    // send
    let blind_data_2 = wallet_2.blind(None, None).unwrap();
    println!("\n=== send 2");
    let recipient_map = HashMap::from([(
        asset_rgb21.asset_id.clone(),
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
        .refresh(online_1.clone(), Some(asset_rgb21.asset_id.clone()))
        .unwrap();
    mine();
    wallet_2.refresh(online_2, None).unwrap();
    wallet_1
        .refresh(online_1, Some(asset_rgb21.asset_id.clone()))
        .unwrap();

    // transfer 2 checks
    println!("check");
    let transfers_w2 = wallet_2
        .list_transfers(asset_rgb21.asset_id.clone())
        .unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = wallet_1
        .list_transfers(asset_rgb21.asset_id.clone())
        .unwrap();
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
        .find(|a| a.asset_id == Some(asset_rgb20.asset_id.clone()))
        .unwrap();
    let ca_a2 = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb21.asset_id.clone()))
        .unwrap();
    assert_eq!(ca_a1.amount, AMOUNT - amount_1);
    assert_eq!(ca_a1.asset_id, Some(asset_rgb20.asset_id.clone()));
    assert!(ca_a1.settled);
    assert_eq!(ca_a2.amount, AMOUNT * 2 - amount_2);
    assert_eq!(ca_a2.asset_id, Some(asset_rgb21.asset_id.clone()));
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
        &asset_rgb20.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint.clone()),
        ca_a1.amount,
    );
    check_state_map_asset_amount(
        &state_map_w1,
        &asset_rgb21.asset_id,
        &OutPoint::from(change_unspent.utxo.outpoint),
        ca_a2.amount,
    );
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
    let asset_rgb20 = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_rgb21 = wallet_1
        .issue_asset_rgb21(
            online_1.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
            Some(file_str.to_string()),
        )
        .unwrap();

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let blind_data_a20 = wallet_2.blind(None, None).unwrap();
    let blind_data_a21 = wallet_2.blind(None, None).unwrap();
    let recipient_map = HashMap::from([
        (
            asset_rgb20.asset_id.clone(),
            vec![Recipient {
                blinded_utxo: blind_data_a20.blinded_utxo.clone(),
                amount: amount_1a,
            }],
        ),
        (
            asset_rgb21.asset_id.clone(),
            vec![Recipient {
                blinded_utxo: blind_data_a21.blinded_utxo.clone(),
                amount: amount_1b,
            }],
        ),
    ]);
    let txid_1 = wallet_1
        .send(online_1.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_1.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1.refresh(online_1.clone(), None).unwrap();
    mine();
    wallet_2.refresh(online_2.clone(), None).unwrap();
    wallet_1.refresh(online_1, None).unwrap();

    // transfer 1 checks
    let (transfers_w1, _, _) = get_test_transfers_sender(&wallet_1, &txid_1);
    let transfers_for_asset_rgb20 = transfers_w1.get(&asset_rgb20.asset_id).unwrap();
    let transfers_for_asset_rgb21 = transfers_w1.get(&asset_rgb21.asset_id).unwrap();
    assert_eq!(transfers_for_asset_rgb20.len(), 1);
    assert_eq!(transfers_for_asset_rgb21.len(), 1);
    let transfer_w1a = transfers_for_asset_rgb20.first().unwrap();
    let transfer_w1b = transfers_for_asset_rgb21.first().unwrap();
    let transfer_w2a = get_test_transfer_recipient(&wallet_2, &blind_data_a20.blinded_utxo);
    let transfer_w2b = get_test_transfer_recipient(&wallet_2, &blind_data_a21.blinded_utxo);
    let transfer_data_w1a = wallet_1.database.get_transfer_data(transfer_w1a).unwrap();
    let transfer_data_w1b = wallet_1.database.get_transfer_data(transfer_w1b).unwrap();
    let transfer_data_w2a = wallet_2.database.get_transfer_data(&transfer_w2a).unwrap();
    let transfer_data_w2b = wallet_2.database.get_transfer_data(&transfer_w2b).unwrap();
    assert_eq!(transfer_w1a.amount, amount_1a.to_string());
    assert_eq!(transfer_w1b.amount, amount_1b.to_string());
    assert_eq!(transfer_w2a.amount, amount_1a.to_string());
    assert_eq!(transfer_w2b.amount, amount_1b.to_string());
    assert_eq!(transfer_data_w1a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w1b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);

    let unspents = wallet_1.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w1a.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    let change_allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb20.asset_id.clone()))
        .unwrap();
    let change_allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb21.asset_id.clone()))
        .unwrap();
    assert_eq!(change_allocations.len(), 2);
    assert_eq!(change_allocation_a.amount, AMOUNT - amount_1a);
    assert_eq!(change_allocation_b.amount, AMOUNT * 2 - amount_1b);

    //
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let blind_data_b20 = wallet_3.blind(None, None).unwrap();
    let blind_data_b21 = wallet_3.blind(None, None).unwrap();
    let recipient_map = HashMap::from([
        (
            asset_rgb20.asset_id.clone(),
            vec![Recipient {
                blinded_utxo: blind_data_b20.blinded_utxo.clone(),
                amount: amount_2a,
            }],
        ),
        (
            asset_rgb21.asset_id.clone(),
            vec![Recipient {
                blinded_utxo: blind_data_b21.blinded_utxo.clone(),
                amount: amount_2b,
            }],
        ),
    ]);
    let txid_2 = wallet_2
        .send(online_2.clone(), recipient_map, false)
        .unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_3.refresh(online_3.clone(), None).unwrap();
    wallet_2.refresh(online_2.clone(), None).unwrap();
    mine();
    wallet_3.refresh(online_3, None).unwrap();
    wallet_2.refresh(online_2, None).unwrap();

    // transfer 2 checks
    let (transfers_w2, _, _) = get_test_transfers_sender(&wallet_2, &txid_2);
    let transfers_for_asset_rgb20 = transfers_w2.get(&asset_rgb20.asset_id).unwrap();
    let transfers_for_asset_rgb21 = transfers_w2.get(&asset_rgb21.asset_id).unwrap();
    assert_eq!(transfers_for_asset_rgb20.len(), 1);
    assert_eq!(transfers_for_asset_rgb21.len(), 1);
    let transfer_w2a = transfers_for_asset_rgb20.first().unwrap();
    let transfer_w2b = transfers_for_asset_rgb21.first().unwrap();
    let transfer_w3a = get_test_transfer_recipient(&wallet_3, &blind_data_b20.blinded_utxo);
    let transfer_w3b = get_test_transfer_recipient(&wallet_3, &blind_data_b21.blinded_utxo);
    let transfer_data_w2a = wallet_2.database.get_transfer_data(transfer_w2a).unwrap();
    let transfer_data_w2b = wallet_2.database.get_transfer_data(transfer_w2b).unwrap();
    let transfer_data_w3a = wallet_3.database.get_transfer_data(&transfer_w3a).unwrap();
    let transfer_data_w3b = wallet_3.database.get_transfer_data(&transfer_w3b).unwrap();
    assert_eq!(transfer_w2a.amount, amount_2a.to_string());
    assert_eq!(transfer_w2b.amount, amount_2b.to_string());
    assert_eq!(transfer_w3a.amount, amount_2a.to_string());
    assert_eq!(transfer_w3b.amount, amount_2b.to_string());
    assert_eq!(transfer_data_w2a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w2b.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3a.status, TransferStatus::Settled);
    assert_eq!(transfer_data_w3b.status, TransferStatus::Settled);

    let unspents = wallet_2.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_w2a.change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    let change_allocation_a = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb20.asset_id.clone()))
        .unwrap();
    let change_allocation_b = change_allocations
        .iter()
        .find(|a| a.asset_id == Some(asset_rgb21.asset_id.clone()))
        .unwrap();
    assert_eq!(change_allocations.len(), 2);
    assert_eq!(change_allocation_a.amount, amount_1a - amount_2a);
    assert_eq!(change_allocation_b.amount, amount_1b - amount_2b);

    // check rgb21 asset has the correct attachment after being received
    let rgb21_assets = wallet_3
        .list_assets(vec![AssetType::Rgb21])
        .unwrap()
        .rgb21
        .unwrap();
    assert_eq!(rgb21_assets.len(), 1);
    let recv_asset = rgb21_assets.first().unwrap();
    let dst_path = recv_asset.data_paths.first().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = AttachmentId::commit(&src_hash).to_string();
    let dst_attachment_id = Path::new(&dst_path)
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert_eq!(src_attachment_id, dst_attachment_id);
}

#[test]
fn send_received_rgb21_success() {
    initialize();

    let amount_1: u64 = 66;
    let amount_2: u64 = 7;
    let parent_str = "mom|dad";
    let file_str = "README.md";

    // wallets
    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();
    let (mut wallet_3, online_3) = get_funded_wallet!();

    // issue
    let asset = wallet_1
        .issue_asset_rgb21(
            online_1.clone(),
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            Some(parent_str.to_string()),
            Some(file_str.to_string()),
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
        .refresh(online_1, Some(asset.asset_id.clone()))
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
    // 2nd transfer: wallet 2 > wallet 3
    //

    // send
    let blind_data_2 = wallet_3.blind(None, None).unwrap();
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
    wallet_3.refresh(online_3.clone(), None).unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    wallet_3.refresh(online_3, None).unwrap();
    wallet_2
        .refresh(online_2, Some(asset.asset_id.clone()))
        .unwrap();

    // transfer 2 checks
    let transfer_w3 = get_test_transfer_recipient(&wallet_3, &blind_data_2.blinded_utxo);
    let (transfer_w2, _, _) = get_test_transfer_sender(&wallet_2, &txid_2);
    let transfer_data_w3 = wallet_3.database.get_transfer_data(&transfer_w3).unwrap();
    let transfer_data_w2 = wallet_2.database.get_transfer_data(&transfer_w2).unwrap();
    assert_eq!(transfer_w3.amount, amount_2.to_string());
    assert_eq!(transfer_w2.amount, amount_2.to_string());
    assert_eq!(transfer_data_w3.status, TransferStatus::Settled);
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
    // check asset has been received correctly
    let rgb21_assets = wallet_3
        .list_assets(vec![AssetType::Rgb21])
        .unwrap()
        .rgb21
        .unwrap();
    assert_eq!(rgb21_assets.len(), 1);
    let recv_asset = rgb21_assets.first().unwrap();
    assert_eq!(recv_asset.asset_id, asset.asset_id);
    assert_eq!(recv_asset.name, NAME.to_string());
    assert_eq!(recv_asset.description, Some(DESCRIPTION.to_string()));
    assert_eq!(recv_asset.precision, PRECISION);
    assert_eq!(
        recv_asset.balance,
        Balance {
            settled: amount_2,
            future: amount_2
        }
    );
    assert_eq!(recv_asset.parent_id, Some(parent_str.to_string()));
    // check attachment data matches
    let dst_path = recv_asset.data_paths.first().unwrap().file_path.clone();
    let src_bytes = std::fs::read(PathBuf::from(file_str)).unwrap();
    let dst_bytes = std::fs::read(PathBuf::from(dst_path.clone())).unwrap();
    assert_eq!(src_bytes, dst_bytes);
    // check attachment id for provided file matches
    let src_hash: sha256::Hash = Sha256Hash::hash(&src_bytes[..]);
    let src_attachment_id = AttachmentId::commit(&src_hash).to_string();
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
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // send
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![
            Recipient {
                amount: amount_1,
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
            },
            Recipient {
                amount: amount_2,
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
            },
        ],
    )]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let rcv_asset_transfer_1 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_1.asset_transfer_idx);
    let rcv_asset_transfer_2 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_2.asset_transfer_idx);
    let (transfers, asset_transfers, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(asset_transfers.len(), 1);
    assert_eq!(transfers.len(), 1);
    let asset_transfer = asset_transfers.first().unwrap();
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_1.blinded_utxo.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_2.blinded_utxo.clone()))
        .unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
    // blinded_utxo is set
    assert_eq!(
        rcv_transfer_1.blinded_utxo,
        Some(blind_data_1.blinded_utxo.clone())
    );
    assert_eq!(
        rcv_transfer_2.blinded_utxo,
        Some(blind_data_2.blinded_utxo.clone())
    );
    assert_eq!(
        transfer_1.blinded_utxo,
        Some(blind_data_1.blinded_utxo.clone())
    );
    assert_eq!(
        transfer_2.blinded_utxo,
        Some(blind_data_2.blinded_utxo.clone())
    );
    // blindind_secret
    assert_eq!(
        rcv_transfer_1.blinding_secret,
        Some(blind_data_1.blinding_secret.to_string())
    );
    assert_eq!(
        rcv_transfer_2.blinding_secret,
        Some(blind_data_2.blinding_secret.to_string())
    );
    assert!(transfer_1.blinding_secret.is_none());
    assert!(transfer_2.blinding_secret.is_none());

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
    assert!(rcv_transfer_data_1.incoming);
    assert!(rcv_transfer_data_2.incoming);
    assert!(!transfer_data_1.incoming);
    assert!(!transfer_data_2.incoming);
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
    // unblinded UTXO is set only for the receiver
    assert!(rcv_transfer_data_1.unblinded_utxo.is_some());
    assert!(rcv_transfer_data_2.unblinded_utxo.is_some());
    assert!(transfer_data_1.unblinded_utxo.is_none());
    assert!(transfer_data_2.unblinded_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer_1.asset_rgb20_id.is_none());
    assert!(rcv_asset_transfer_1.asset_rgb21_id.is_none());
    assert!(rcv_asset_transfer_2.asset_rgb20_id.is_none());
    assert!(rcv_asset_transfer_2.asset_rgb21_id.is_none());
    assert_eq!(asset_transfer.asset_rgb20_id, Some(asset.asset_id.clone()));
    assert_eq!(asset_transfer.asset_rgb21_id, None);
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer_1.user_driven);
    assert!(rcv_asset_transfer_2.user_driven);
    assert!(asset_transfer.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let rcv_asset_transfer_1 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_1.asset_transfer_idx);
    let rcv_asset_transfer_2 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_2.asset_transfer_idx);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 1);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_1.blinded_utxo.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_2.blinded_utxo.clone()))
        .unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
        rcv_asset_transfer_1.asset_rgb20_id,
        Some(asset.asset_id.clone())
    );
    assert_eq!(
        rcv_asset_transfer_2.asset_rgb20_id,
        Some(asset.asset_id.clone())
    );
    assert_eq!(rcv_asset_transfer_1.asset_rgb21_id, None);
    assert_eq!(rcv_asset_transfer_2.asset_rgb21_id, None);
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
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()))
        .unwrap();

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 1);
    let transfers_for_asset = transfers.get(&asset.asset_id).unwrap();
    assert_eq!(transfers_for_asset.len(), 2);
    let transfer_1 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_1.blinded_utxo.clone()))
        .unwrap();
    let transfer_2 = transfers_for_asset
        .iter()
        .find(|t| t.blinded_utxo == Some(blind_data_2.blinded_utxo.clone()))
        .unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
    wallet._sync_db_txos().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
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
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_2 = wallet
        .issue_asset_rgb21(
            online.clone(),
            s!("NAME2"),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 2],
            None,
            None,
        )
        .unwrap();

    // send
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([
        (
            asset_1.asset_id.clone(),
            vec![Recipient {
                amount: amount_1,
                blinded_utxo: blind_data_1.blinded_utxo.clone(),
            }],
        ),
        (
            asset_2.asset_id.clone(),
            vec![Recipient {
                amount: amount_2,
                blinded_utxo: blind_data_2.blinded_utxo.clone(),
            }],
        ),
    ]);
    let txid = wallet.send(online.clone(), recipient_map, false).unwrap();
    assert!(!txid.is_empty());

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let rcv_asset_transfer_1 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_1.asset_transfer_idx);
    let rcv_asset_transfer_2 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_2.asset_transfer_idx);
    let (transfers, asset_transfers, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(asset_transfers.len(), 2);
    assert_eq!(transfers.len(), 2);
    let asset_transfer_1 = asset_transfers
        .iter()
        .find(|a| a.asset_rgb20_id == Some(asset_1.asset_id.clone()))
        .unwrap();
    let asset_transfer_2 = asset_transfers
        .iter()
        .find(|a| a.asset_rgb21_id == Some(asset_2.asset_id.clone()))
        .unwrap();
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
    // blinded_utxo is set
    assert_eq!(
        rcv_transfer_1.blinded_utxo,
        Some(blind_data_1.blinded_utxo.clone())
    );
    assert_eq!(
        rcv_transfer_2.blinded_utxo,
        Some(blind_data_2.blinded_utxo.clone())
    );
    assert_eq!(
        transfer_1.blinded_utxo,
        Some(blind_data_1.blinded_utxo.clone())
    );
    assert_eq!(
        transfer_2.blinded_utxo,
        Some(blind_data_2.blinded_utxo.clone())
    );
    // blinding_secret
    assert_eq!(
        rcv_transfer_1.blinding_secret,
        Some(blind_data_1.blinding_secret.to_string())
    );
    assert_eq!(
        rcv_transfer_2.blinding_secret,
        Some(blind_data_2.blinding_secret.to_string())
    );
    assert!(transfer_1.blinding_secret.is_none());
    assert!(transfer_2.blinding_secret.is_none());

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
    assert!(rcv_transfer_data_1.incoming);
    assert!(rcv_transfer_data_2.incoming);
    assert!(!transfer_data_1.incoming);
    assert!(!transfer_data_2.incoming);
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
    // unblinded UTXO is set only for the receiver
    assert!(rcv_transfer_data_1.unblinded_utxo.is_some());
    assert!(rcv_transfer_data_2.unblinded_utxo.is_some());
    assert!(transfer_data_1.unblinded_utxo.is_none());
    assert!(transfer_data_2.unblinded_utxo.is_none());

    // asset id is set only for the sender
    assert!(rcv_asset_transfer_1.asset_rgb20_id.is_none());
    assert!(rcv_asset_transfer_1.asset_rgb21_id.is_none());
    assert!(rcv_asset_transfer_2.asset_rgb20_id.is_none());
    assert!(rcv_asset_transfer_2.asset_rgb21_id.is_none());
    assert_eq!(
        asset_transfer_1.asset_rgb20_id,
        Some(asset_1.asset_id.clone())
    );
    assert_eq!(asset_transfer_1.asset_rgb21_id, None);
    assert_eq!(asset_transfer_2.asset_rgb20_id, None);
    assert_eq!(
        asset_transfer_2.asset_rgb21_id,
        Some(asset_2.asset_id.clone())
    );
    // transfers are user-driven on both sides
    assert!(rcv_asset_transfer_1.user_driven);
    assert!(rcv_asset_transfer_2.user_driven);
    assert!(asset_transfer_1.user_driven);
    assert!(asset_transfer_2.user_driven);

    // transfers progress to status WaitingConfirmations after a refresh
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset_1.asset_id.clone()))
        .unwrap();

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let rcv_asset_transfer_1 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_1.asset_transfer_idx);
    let rcv_asset_transfer_2 =
        get_test_asset_transfer(&rcv_wallet, rcv_transfer_2.asset_transfer_idx);
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
        rcv_asset_transfer_1.asset_rgb20_id,
        Some(asset_1.asset_id.clone())
    );
    assert_eq!(rcv_asset_transfer_1.asset_rgb21_id, None);
    assert_eq!(rcv_asset_transfer_2.asset_rgb20_id, None);
    assert_eq!(
        rcv_asset_transfer_2.asset_rgb21_id,
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
    let rgb20_assets = rcv_assets.rgb20.unwrap();
    let rgb21_assets = rcv_assets.rgb21.unwrap();
    assert_eq!(rgb20_assets.len(), 1);
    assert_eq!(rgb21_assets.len(), 1);
    let rcv_asset_rgb20 = rgb20_assets.last().unwrap();
    assert_eq!(rcv_asset_rgb20.asset_id, asset_1.asset_id);
    assert_eq!(rcv_asset_rgb20.ticker, TICKER);
    assert_eq!(rcv_asset_rgb20.name, NAME);
    assert_eq!(rcv_asset_rgb20.precision, PRECISION);
    assert_eq!(
        rcv_asset_rgb20.balance,
        Balance {
            settled: 0,
            future: amount_1
        }
    );
    let rcv_asset_rgb21 = rgb21_assets.last().unwrap();
    assert_eq!(rcv_asset_rgb21.asset_id, asset_2.asset_id);
    assert_eq!(rcv_asset_rgb21.name, s!("NAME2"));
    assert_eq!(rcv_asset_rgb21.description, Some(DESCRIPTION.to_string()));
    assert_eq!(rcv_asset_rgb21.precision, PRECISION);
    assert_eq!(
        rcv_asset_rgb21.balance,
        Balance {
            settled: 0,
            future: amount_2
        }
    );
    assert!(rcv_asset_rgb21.parent_id.is_none());
    let empty_data_paths = vec![];
    assert_eq!(rcv_asset_rgb21.data_paths, empty_data_paths);

    // transfers progress to status Settled after tx mining + refresh
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet
        .refresh(online, Some(asset_1.asset_id.clone()))
        .unwrap();

    let rcv_transfer_1 = get_test_transfer_recipient(&rcv_wallet, &blind_data_1.blinded_utxo);
    let rcv_transfer_2 = get_test_transfer_recipient(&rcv_wallet, &blind_data_2.blinded_utxo);
    let rcv_transfer_data_1 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_1)
        .unwrap();
    let rcv_transfer_data_2 = rcv_wallet
        .database
        .get_transfer_data(&rcv_transfer_2)
        .unwrap();
    let (transfers, _, _) = get_test_transfers_sender(&wallet, &txid);
    assert_eq!(transfers.len(), 2);
    let transfers_for_asset_1 = transfers.get(&asset_1.asset_id).unwrap();
    let transfers_for_asset_2 = transfers.get(&asset_2.asset_id).unwrap();
    assert_eq!(transfers_for_asset_1.len(), 1);
    assert_eq!(transfers_for_asset_2.len(), 1);
    let transfer_1 = transfers_for_asset_1.first().unwrap();
    let transfer_2 = transfers_for_asset_2.first().unwrap();
    let transfer_data_1 = wallet.database.get_transfer_data(transfer_1).unwrap();
    let transfer_data_2 = wallet.database.get_transfer_data(transfer_2).unwrap();

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
    wallet._sync_db_txos().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| Some(u.utxo.outpoint.clone()) == transfer_data_1.change_utxo);
    assert!(change_unspent.is_some());
}

#[test]
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
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
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_b = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_c = wallet
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
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
        .issue_asset_rgb20(
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
        .issue_asset_rgb20(
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
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
fn pending_transfer_input_fail() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // blind with sender wallet to create a pending transfer
    wallet.blind(None, None).unwrap();

    // send and check it fails as the issuance utxo is "blocked" by the pending receive operation
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
        .issue_asset_rgb20(
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

#[test]
fn rgb21_blank_success() {
    initialize();

    let amount_issue_ft = 10000;
    let amount_issue_nft = 1;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // issue rgb20
    let asset_rgb20 = wallet
        .issue_asset_rgb20(
            online.clone(),
            s!("TFT"),
            s!("Test Fungible Token"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // issue rgb21
    let _asset_rgb21 = wallet
        .issue_asset_rgb21(
            online.clone(),
            s!("Test Non Funguble Token"),
            Some(s!("Debugging rgb blank error")),
            PRECISION,
            vec![amount_issue_nft],
            None,
            Some(s!("README.md")),
        )
        .unwrap();

    let unspents = wallet.list_unspents(false).unwrap();

    let blind_data = rcv_wallet.blind(None, None).unwrap();

    // try sending rgb20
    let recipient_map = HashMap::from([(
        asset_rgb20.asset_id,
        vec![Recipient {
            amount: 1,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    let res = wallet.send_begin(online, recipient_map, false);
    dbg!(&res);
    assert!(!res.unwrap().is_empty());
}

#[test]
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
fn psbt_rgb_consumer_success() {
    initialize();

    let amount_issue_ft = 10000;

    // create wallet with funds and no utxos
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_funded_wallet!();

    // create 1 utxo
    println!("utxo 1");
    let num_utxos_created = wallet.create_utxos(online.clone(), true, Some(1)).unwrap();
    assert_eq!(num_utxos_created, 1);

    // issue an rgb20 asset
    println!("issue 1");
    let asset_rgb20_a = wallet
        .issue_asset_rgb20(
            online.clone(),
            s!("TFT1"),
            s!("Test Fungible Token 1"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // create 1 more utxo for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 2");
    let num_utxos_created = wallet.create_utxos(online.clone(), false, Some(1)).unwrap();
    assert_eq!(num_utxos_created, 1);

    // try to send it
    println!("send_begin 1");
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_rgb20_a.asset_id,
        vec![Recipient {
            amount: 1,
            blinded_utxo: blind_data_1.blinded_utxo,
        }],
    )]);
    let res = wallet.send_begin(online.clone(), recipient_map, false);
    if res.is_err() {
        dbg!(&res);
    }
    assert!(!res.unwrap().is_empty());

    // issue one more rgb20 asset, should go to the same utxo as the 1st issuance
    println!("issue 2");
    let asset_rgb20_b = wallet
        .issue_asset_rgb20(
            online.clone(),
            s!("TFT2"),
            s!("Test Fungible Token 2"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();

    // try to send the second asset
    println!("send_begin 2");
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_rgb20_b.asset_id.clone(),
        vec![Recipient {
            amount: 1,
            blinded_utxo: blind_data_2.blinded_utxo,
        }],
    )]);
    let res = wallet.send_begin(online.clone(), recipient_map, false);
    if res.is_err() {
        dbg!(&res);
    }
    assert!(!res.unwrap().is_empty());

    // exhaust allocations + issue 3rd asset, on a different UTXO
    println!("exhaust allocations on current UTXO");
    let new_allocation_count = (MAX_ALLOCATIONS_PER_UTXO as i64 - 2).max(0);
    for _ in 0..new_allocation_count {
        let _blind_data = wallet.blind(None, None).unwrap();
    }
    println!("issue 3");
    let asset_rgb20_c = wallet
        .issue_asset_rgb20(
            online.clone(),
            s!("TFT3"),
            s!("Test Fungible Token 3"),
            PRECISION,
            vec![amount_issue_ft],
        )
        .unwrap();
    // fail transfers so 1st UTXO can be used as input
    wallet.fail_transfers(online.clone(), None, None).unwrap();

    // create 1 more utxo for change, up_to false or AllocationsAlreadyAvailable is returned
    println!("utxo 3");
    let num_utxos_created = wallet.create_utxos(online.clone(), false, Some(1)).unwrap();
    assert_eq!(num_utxos_created, 1);

    // try to send the second asset to a recipient and the third to different one
    println!("send_begin 3");
    let blind_data_3a = rcv_wallet.blind(None, None).unwrap();
    let blind_data_3b = rcv_wallet.blind(None, None).unwrap();
    let recipient_map = HashMap::from([
        (
            asset_rgb20_b.asset_id,
            vec![Recipient {
                amount: 1,
                blinded_utxo: blind_data_3a.blinded_utxo,
            }],
        ),
        (
            asset_rgb20_c.asset_id,
            vec![Recipient {
                amount: 1,
                blinded_utxo: blind_data_3b.blinded_utxo,
            }],
        ),
    ]);
    let res = wallet.send_begin(online, recipient_map, false);
    if res.is_err() {
        dbg!(&res);
    }
    assert!(!res.unwrap().is_empty());
}
