use super::*;

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
    let result = offline_party.wallet.sync(
        Online { id: 0 },
        SyncOptions {
            keychain: SyncKeychain::Colored,
            strategy: SyncStrategy::FastSync,
        },
    );
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let sync_options = SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FastSync,
    };

    // wallets
    let mut party = get_funded_party!();

    // sync input params
    // - check online is correct
    let wrong_online = Online { id: 1 };
    let good_online = party.online;
    party.online = wrong_online;
    let result = party.sync_result(sync_options);
    party.online = good_online;
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_sync_vanilla() {
    initialize();

    let amount = 10_000;
    let vanilla_balance = |amount: u64| BtcBalance {
        vanilla: Balance {
            settled: amount,
            future: amount,
            spendable: amount,
        },
        colored: Balance::default(),
    };

    // the pinned address sits past the stop gap and the lookback window
    let mut party = get_empty_party!();
    for _ in 0..=INDEXER_STOP_GAP {
        party.wallet.get_address(AddressReuse::New).unwrap();
    }
    let pinned = party.wallet.pin_address(Keychain::Vanilla, None).unwrap();
    for _ in 0..=INDEXER_SYNC_LOOKBACK {
        party.wallet.get_address(AddressReuse::New).unwrap();
    }

    // a payment past the lookback window is seen
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    party.wait_for_btc_balance(&vanilla_balance(amount));

    // a payment after every earlier output is spent is seen
    party.drain_wallet();
    mine(false);
    party.wait_for_btc_balance(&vanilla_balance(0));
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    party.wait_for_btc_balance(&vanilla_balance(amount));

    // a payment made while the wallet is closed is seen after reload
    let keys = party.wallet.get_keys();
    let data_dir = party.wallet.get_wallet_data().data_dir;
    drop(party);
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    let mut wallet = Wallet::load(&data_dir, &keys.master_fingerprint, keys.mnemonic).unwrap();
    let mut online_options = test_go_online_options(None);
    online_options.skip_consistency_check = false;
    let online = wallet.go_online(online_options).unwrap();
    assert_eq!(
        wallet.get_btc_balance(Some(online), true).unwrap(),
        vanilla_balance(amount * 2)
    );
    assert_eq!(wallet.get_address(AddressReuse::Pinned).unwrap(), pinned);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_sync_colored() {
    initialize();

    let amount = 10_000;
    let asset_amount = 100;

    // the pinned address sits past the stop gap
    let mut rcv_party = get_empty_party!();
    for _ in 0..=INDEXER_STOP_GAP {
        rcv_party.wallet.get_new_address().unwrap();
    }
    let pinned = rcv_party
        .wallet
        .pin_address(Keychain::Colored, None)
        .unwrap();
    for _ in 0..=INDEXER_STOP_GAP {
        rcv_party.wallet.get_new_address().unwrap();
    }
    let pinned_script = rcv_party.wallet.get_script_pubkey(&pinned).unwrap();
    // return the outputs on the pinned address, as the given unspents show them
    let pinned_outpoints = |unspents: Vec<Unspent>, party: &SinglesigParty| {
        unspents
            .into_iter()
            .filter(|u| {
                party
                    .wallet
                    .bdk_wallet()
                    .get_utxo(u.utxo.outpoint.clone().into())
                    .is_some_and(|o| o.txout.script_pubkey == pinned_script)
            })
            .map(|u| u.utxo.outpoint)
            .collect::<Vec<Outpoint>>()
    };
    let wait_pinned_outpoints = |party: &mut SinglesigParty, len: usize| {
        let mut outpoints = vec![];
        let check = || {
            let unspents = party.list_unspents_with_sync(false);
            outpoints = pinned_outpoints(unspents, party);
            outpoints.len() == len
        };
        assert!(wait_for_function(check, 10, 500));
        outpoints
    };

    // plain sats are seen and quarantined
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    let outpoints = wait_pinned_outpoints(&mut rcv_party, 1);
    assert!(rcv_party.db_txo(&outpoints[0]).unwrap().pending_witness);

    // a payment after every earlier output is spent is seen
    rcv_party.drain_wallet();
    mine(false);
    wait_pinned_outpoints(&mut rcv_party, 0);
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    let outpoints = wait_pinned_outpoints(&mut rcv_party, 1);
    assert!(rcv_party.db_txo(&outpoints[0]).unwrap().pending_witness);

    // an RGB receive on the pinned address settles on its own outpoint
    let mut party = get_funded_party!();
    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.witness_receive_reuse(
        Assignment::Any,
        TRANSPORT_ENDPOINTS.clone(),
        default_rcv_expiration(),
        AddressReuse::Pinned,
    );
    party.send_retry(&HashMap::from([(
        asset.asset_id.clone(),
        vec![witness_recipient(
            &receive_data.recipient_id,
            asset_amount,
            TRANSPORT_ENDPOINTS.clone(),
        )],
    )]));
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);
    rcv_party.wait_for_refresh(None);
    let transfer = rcv_party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (transfer_data, _) = rcv_party.get_test_transfer_data(&transfer);
    assert_eq!(transfer_data.status, TransferStatus::Settled);
    let receive_utxo = transfer_data.receive_utxo.unwrap();
    assert!(!rcv_party.db_txo(&receive_utxo).unwrap().pending_witness);
    assert!(rcv_party.db_txo(&outpoints[0]).unwrap().pending_witness);
    let unspents = rcv_party.list_unspents(false);
    let unspent = unspents
        .iter()
        .find(|u| u.utxo.outpoint == receive_utxo)
        .unwrap();
    assert_eq!(
        unspent.rgb_allocations.first().unwrap().assignment,
        Assignment::Fungible(asset_amount)
    );
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        asset_amount
    );

    // a payment made while the wallet is closed is seen after reload
    let keys = rcv_party.wallet.get_keys();
    let data_dir = rcv_party.wallet.get_wallet_data().data_dir;
    drop(rcv_party);
    send_sats_to_address(pinned.clone(), Some(amount));
    mine(false);
    let mut wallet = Wallet::load(&data_dir, &keys.master_fingerprint, keys.mnemonic).unwrap();
    let mut online_options = test_go_online_options(None);
    online_options.skip_consistency_check = false;
    let online = wallet.go_online(online_options).unwrap();
    let mut rcv_party = party!(wallet, online);
    let unspents = rcv_party.wallet.list_unspents(None, false, true).unwrap();
    let outpoints = pinned_outpoints(unspents, &rcv_party);
    assert_eq!(outpoints.len(), 3);
    let flagged = outpoints
        .iter()
        .filter(|o| rcv_party.db_txo(o).unwrap().pending_witness)
        .count();
    assert_eq!(flagged, 2);
}
