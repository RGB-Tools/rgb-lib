use super::*;

#[test]
fn success() {
    initialize();

    let amount: u64 = 66;

    fn _sender_test_db_transfer(wallet: &Wallet) -> DbTransfer {
        wallet
            .database
            .iter_transfers()
            .unwrap()
            .into_iter()
            .filter(|t| t.blinded_utxo != None) // exclude issuance
            .collect::<Vec<DbTransfer>>()
            .first()
            .unwrap()
            .clone()
    }
    fn _receiver_test_db_transfer(wallet: &Wallet) -> DbTransfer {
        wallet
            .database
            .iter_transfers()
            .unwrap()
            .first()
            .unwrap()
            .clone()
    }

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
            AMOUNT,
        )
        .unwrap();

    // send
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    let txid = wallet
        .send(
            online.clone(),
            asset.asset_id.clone(),
            blind_data.blinded_utxo,
            amount,
        )
        .unwrap();
    assert!(!txid.is_empty());

    // transfers start in status WaitingCounterparty
    let rcv_db_transfer = _receiver_test_db_transfer(&rcv_wallet);
    let db_transfer = _sender_test_db_transfer(&wallet);
    assert_eq!(rcv_db_transfer.status, TransferStatus::WaitingCounterparty);
    assert_eq!(db_transfer.status, TransferStatus::WaitingCounterparty);
    // create and update timestamps are the same
    assert_eq!(rcv_db_transfer.created_at, rcv_db_transfer.updated_at);
    assert_eq!(db_transfer.created_at, db_transfer.updated_at);
    // expiration is create timestamp + expiration offset
    assert_eq!(
        rcv_db_transfer.expiration,
        Some(rcv_db_transfer.created_at + DURATION_RCV_TRANSFER as i64)
    );
    assert_eq!(
        db_transfer.expiration,
        Some(db_transfer.created_at + DURATION_SEND_TRANSFER)
    );
    // asset id is set only on the sender side
    assert!(rcv_db_transfer.asset_id.is_none());
    assert!(db_transfer.asset_id.is_some());
    // txid is set only on the sender side
    assert!(rcv_db_transfer.txid.is_none());
    assert!(db_transfer.txid.is_some());
    // blinding secret is set only on the receiver side
    assert!(rcv_db_transfer.blinding_secret.is_some());
    assert!(db_transfer.blinding_secret.is_none());

    // transfers progress to status WaitingConfirmations after a refresh
    // update timestamp is now later than creation
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    let rcv_db_transfer = _receiver_test_db_transfer(&rcv_wallet);
    assert_eq!(rcv_db_transfer.status, TransferStatus::WaitingConfirmations);
    assert!(rcv_db_transfer.updated_at > rcv_db_transfer.created_at);
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    let db_transfer = _sender_test_db_transfer(&wallet);
    assert_eq!(db_transfer.status, TransferStatus::WaitingConfirmations);
    assert!(db_transfer.updated_at > db_transfer.created_at);

    // use list_transfers() to check additional fields
    // amounts, outgoing for sender, incoming for receiver
    let rcv_transfers = rcv_wallet.list_transfers(asset.asset_id.clone()).unwrap();
    let rcv_transfer = rcv_transfers.first().unwrap();
    assert_eq!(rcv_transfer.received, amount);
    let transfers = wallet.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer = transfers.last().unwrap();
    // unblinded_utxo is set only on the receiver side
    assert!(rcv_transfer.unblinded_utxo.is_some());
    assert!(transfer.unblinded_utxo.is_none());
    // change_utxo is set only on the sender side
    let change = transfer.change_utxo.as_ref().unwrap();
    assert!(rcv_transfer.change_utxo.is_none());
    assert!(transfer.change_utxo.is_some());

    // transfers progress to status Settled after tx mining + refresh
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    let rcv_db_transfer = _receiver_test_db_transfer(&rcv_wallet);
    assert_eq!(rcv_db_transfer.status, TransferStatus::Settled);
    wallet
        .refresh(online, Some(asset.asset_id.clone()))
        .unwrap();
    let db_transfer = _sender_test_db_transfer(&wallet);
    assert_eq!(db_transfer.status, TransferStatus::Settled);

    // change is unspent once transfer is Settled
    wallet._sync_db_txos().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents.into_iter().find(|u| u.utxo.outpoint == *change);
    assert!(change_unspent.is_some());

    // receiver amount has been set
    let rcv_transfers = rcv_wallet.list_transfers(asset.asset_id).unwrap();
    let rcv_transfer = rcv_transfers.first().unwrap();
    assert_eq!(rcv_transfer.received, amount);
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
            AMOUNT,
        )
        .unwrap();

    //
    // 1st transfer
    //

    // send
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let txid_1 = wallet
        .send(
            online.clone(),
            asset.asset_id.clone(),
            blind_data_1.blinded_utxo,
            amount_1,
        )
        .unwrap();
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
    let transfers = wallet.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer = transfers.last().unwrap();
    let rcv_transfers = rcv_wallet.list_transfers(asset.asset_id.clone()).unwrap();
    let rcv_transfer = rcv_transfers.last().unwrap();
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.status, TransferStatus::Settled);
    assert_eq!(transfer.sent - transfer.received, amount_1);
    assert_eq!(rcv_transfer.received, amount_1);
    let change_utxo = transfer.change_utxo.as_ref().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
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
    let txid_2 = wallet
        .send(
            online.clone(),
            asset.asset_id.clone(),
            blind_data_2.blinded_utxo,
            amount_2,
        )
        .unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet
        .refresh(online.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet
        .refresh(online, Some(asset.asset_id.clone()))
        .unwrap();

    // transfer 2 checks
    let transfers = wallet.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer = transfers.last().unwrap();
    let rcv_transfers = rcv_wallet.list_transfers(asset.asset_id).unwrap();
    let rcv_transfer = rcv_transfers.last().unwrap();
    assert_eq!(transfer.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer.status, TransferStatus::Settled);
    assert_eq!(transfer.sent - transfer.received, amount_2);
    assert_eq!(rcv_transfer.sent, 0);
    assert_eq!(rcv_transfer.received, amount_2);
    let change_utxo = transfer.change_utxo.as_ref().unwrap();
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        AMOUNT - amount_1 - amount_2
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
            AMOUNT,
        )
        .unwrap();

    //
    // 1st transfer: wallet 1 > wallet 2
    //

    // send
    let blind_data_1 = wallet_2.blind(None, None).unwrap();
    let txid_1 = wallet_1
        .send(
            online_1.clone(),
            asset.asset_id.clone(),
            blind_data_1.blinded_utxo,
            amount_1,
        )
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
    let transfers_w1 = wallet_1.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    let transfers_w2 = wallet_2.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.sent - transfer_w1.received, amount_1);
    assert_eq!(transfer_w2.received, amount_1);
    let change_utxo = transfer_w1.change_utxo.as_ref().unwrap();
    let unspents = wallet_1.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
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
    let txid_2 = wallet_2
        .send(
            online_2.clone(),
            asset.asset_id.clone(),
            blind_data_2.blinded_utxo,
            amount_2,
        )
        .unwrap();
    assert!(!txid_2.is_empty());

    // take transfers from WaitingCounterparty to Settled
    wallet_1.refresh(online_1.clone(), None).unwrap();
    wallet_2
        .refresh(online_2.clone(), Some(asset.asset_id.clone()))
        .unwrap();
    mine();
    wallet_1.refresh(online_1, None).unwrap();
    wallet_2
        .refresh(online_2, Some(asset.asset_id.clone()))
        .unwrap();

    // transfer 2 checks
    let transfers_w2 = wallet_2.list_transfers(asset.asset_id.clone()).unwrap();
    let transfer_w2 = transfers_w2.last().unwrap();
    let transfers_w1 = wallet_1.list_transfers(asset.asset_id).unwrap();
    let transfer_w1 = transfers_w1.last().unwrap();
    assert_eq!(transfer_w2.status, TransferStatus::Settled);
    assert_eq!(transfer_w1.status, TransferStatus::Settled);
    assert_eq!(transfer_w2.sent - transfer_w2.received, amount_2);
    assert_eq!(transfer_w1.sent, 0);
    assert_eq!(transfer_w1.received, amount_2);
    let change_utxo = transfer_w2.change_utxo.as_ref().unwrap();
    let unspents = wallet_2.list_unspents(true).unwrap();
    let change_unspent = unspents
        .into_iter()
        .find(|u| u.utxo.outpoint == *change_utxo)
        .unwrap();
    let change_allocations = change_unspent.rgb_allocations;
    assert_eq!(change_allocations.len(), 1);
    assert_eq!(
        change_allocations.first().unwrap().amount,
        amount_1 - amount_2
    );
}

#[test]
#[ignore = "requires MAX_ALLOCATIONS_PER_UTXO > 1"]
fn multiple_assets_success() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!(true, true);
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue two assets
    let asset_1 = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();
    let asset_2 = wallet
        .issue_asset(
            online.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION,
            AMOUNT * 2,
        )
        .unwrap();

    // check both assets are allocated to the same utxo
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
    assert!(allocation_asset_ids.contains(&asset_1.asset_id));
    assert!(allocation_asset_ids.contains(&asset_2.asset_id));

    // send some of the first asset
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let txid = wallet
        .send(
            online.clone(),
            asset_1.asset_id.clone(),
            blind_data.blinded_utxo,
            amount,
        )
        .unwrap();
    assert!(!txid.is_empty());

    // settle transfer process
    rcv_wallet.refresh(rcv_online.clone(), None).unwrap();
    wallet.refresh(online.clone(), None).unwrap();
    mine();
    rcv_wallet.refresh(rcv_online, None).unwrap();
    wallet.refresh(online, None).unwrap();

    let transfers_1 = wallet.list_transfers(asset_1.asset_id.clone()).unwrap();
    let transfer_1 = transfers_1.last().unwrap();
    let rcv_transfers_1 = rcv_wallet.list_transfers(asset_1.asset_id.clone()).unwrap();
    let rcv_transfer_1 = rcv_transfers_1.first().unwrap();

    // check transfer amounts
    assert_eq!(transfer_1.sent - transfer_1.received, amount);
    assert_eq!(rcv_transfer_1.received, amount);

    // check final transfer statuses
    assert_eq!(transfer_1.status, TransferStatus::Settled);
    assert_eq!(rcv_transfer_1.status, TransferStatus::Settled);

    // check change utxo
    let change = transfer_1.change_utxo.as_ref().unwrap();
    assert!(transfer_1.change_utxo.is_some());
    let unspents = wallet.list_unspents(true).unwrap();
    let change_unspent = unspents.into_iter().find(|u| u.utxo.outpoint == *change);
    assert!(change_unspent.is_some());

    // check change utxo has allocations for all original assets
    let allocation_asset_ids: Vec<String> = change_unspent
        .unwrap()
        .rgb_allocations
        .into_iter()
        .map(|a| a.asset_id.unwrap_or_else(|| s!("")))
        .collect();
    assert!(allocation_asset_ids.contains(&asset_1.asset_id));
    assert!(allocation_asset_ids.contains(&asset_2.asset_id));
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
            AMOUNT,
        )
        .unwrap();
    // blind
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();

    // invalid input (asset id)
    let result = wallet.send(
        online.clone(),
        s!("invalid"),
        blind_data.blinded_utxo.clone(),
        AMOUNT / 2,
    );
    assert!(matches!(result, Err(Error::AssetNotFound(_))));

    // invalid input (blinded utxo)
    let result = wallet.send(
        online.clone(),
        asset.asset_id.clone(),
        s!("invalid"),
        AMOUNT / 2,
    );
    assert!(matches!(result, Err(Error::InvalidBlindedUTXO(_))));

    // insufficient assets (amount too big)
    let result = wallet.send(online, asset.asset_id, blind_data.blinded_utxo, AMOUNT + 1);
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();

    //
    // 1st transfer
    //

    // send
    let blind_data_1 = rcv_wallet.blind(None, None).unwrap();
    let txid_1 = wallet
        .send(
            online.clone(),
            asset.asset_id.clone(),
            blind_data_1.blinded_utxo,
            amount_1,
        )
        .unwrap();
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

    //
    // 2nd transfer
    //

    // send
    let blind_data_2 = rcv_wallet.blind(None, None).unwrap();
    let txid_2 = wallet
        .send(
            online,
            asset.asset_id.clone(),
            blind_data_2.blinded_utxo,
            amount_2,
        )
        .unwrap();
    assert!(!txid_2.is_empty());

    // send from receiving wallet, 1st receive Settled, 2nd one still pending
    let blind_data = wallet.blind(None, None).unwrap();
    let result = rcv_wallet.send(
        rcv_online,
        asset.asset_id,
        blind_data.blinded_utxo,
        amount_3,
    );
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
            AMOUNT,
        )
        .unwrap();

    // 1st send
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let txid = wallet
        .send(
            online.clone(),
            asset.asset_id.clone(),
            blind_data.blinded_utxo,
            amount,
        )
        .unwrap();
    assert!(!txid.is_empty());

    // 2nd send (1st still pending)
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let result = wallet.send(online, asset.asset_id, blind_data.blinded_utxo, amount / 2);
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
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();

    // blind with sender wallet to create a pending transfer
    wallet.blind(None, None).unwrap();

    // send anche check it fails as the issuance utxo is "blocked" by the pending receive operation
    let blind_data = rcv_wallet.blind(None, None).unwrap();
    let result = wallet.send(online, asset.asset_id, blind_data.blinded_utxo, amount);
    assert!(matches!(result, Err(Error::InsufficientAssets)));
}

#[test]
fn send_begin_success() {
    initialize();

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
            AMOUNT,
        )
        .unwrap();

    // blind
    let blind_data = rcv_wallet.blind(None, Some(60)).unwrap();

    // can call send_begin twice as transfer is added to db on send_end()
    let txid = wallet
        .send_begin(
            online.clone(),
            asset.asset_id.clone(),
            blind_data.blinded_utxo.clone(),
            66,
        )
        .unwrap();
    assert!(!txid.is_empty());
    let txid = wallet
        .send_begin(online, asset.asset_id, blind_data.blinded_utxo, 66)
        .unwrap();
    assert!(!txid.is_empty());
}
