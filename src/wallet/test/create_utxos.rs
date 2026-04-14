use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // up_to version with 0 allocatable UTXOs
    println!("\n=== up_to true, 0 allocatable");
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();
    test_create_utxos(&mut wallet, online, true, None, None, FEE_RATE, None);
    let bak_info_after = wallet.database().get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // up_to version with allocatable UTXOs partially available (1 missing)
    println!("\n=== up_to true, need to create 1 more");
    test_create_utxos(
        &mut wallet,
        online,
        true,
        Some(UTXO_NUM + 1),
        None,
        FEE_RATE,
        Some(1),
    );
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 2) as usize);

    // forced version always creates UTXOs
    println!("\n=== up_to false");
    test_create_utxos(&mut wallet, online, false, None, None, FEE_RATE, None);
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM * 2 + 2) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn up_to_allocation_checks() {
    initialize();

    let amount = 66;

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // MAX_ALLOCATIONS_PER_UTXO failed allocations
    //  - check unspent counted as allocatable
    test_create_utxos(&mut wallet, online, false, Some(1), None, FEE_RATE, None);
    let mut batch_transfer_idxs: Vec<i32> = vec![];
    let mut txo_list: HashSet<Outpoint> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = test_blind_receive(&mut wallet);
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let txo =
            if let RecipientTypeFull::Blind { unblinded_utxo } = transfer.recipient_type.unwrap() {
                unblinded_utxo
            } else {
                panic!("should be a Blind variant");
            };
        batch_transfer_idxs.push(receive_data.batch_transfer_idx);
        txo_list.insert(txo);
    }
    // check all transfers are on the same UTXO + fail all of them
    assert_eq!(txo_list.len(), 1);
    for batch_transfer_idx in batch_transfer_idxs {
        assert!(test_fail_transfers_single(
            &mut wallet,
            online,
            batch_transfer_idx
        ));
    }
    // request 1 new UTXO, expecting the existing one is still allocatable
    let result = wallet.create_utxos(online, true, Some(1), None, FEE_RATE, false);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 2);

    // new wallet
    let (mut wallet, online) = get_funded_noutxo_wallet!();

    // MAX_ALLOCATIONS_PER_UTXO allocations
    test_create_utxos(&mut wallet, online, true, Some(1), None, FEE_RATE, None);
    // create MAX_ALLOCATIONS_PER_UTXO blinds on the same UTXO
    let mut txo_list: HashSet<Outpoint> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = test_blind_receive(&mut wallet);
        let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
        let txo =
            if let RecipientTypeFull::Blind { unblinded_utxo } = transfer.recipient_type.unwrap() {
                unblinded_utxo
            } else {
                panic!("should be a Blind variant");
            };
        txo_list.insert(txo);
    }
    assert_eq!(txo_list.len(), 1);
    // request 1 new UTXO, expecting one is created
    test_create_utxos(&mut wallet, online, true, Some(1), None, FEE_RATE, None);
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 3);

    if MAX_ALLOCATIONS_PER_UTXO > 2 {
        // new wallet
        let (mut wallet, online) = get_funded_noutxo_wallet!();
        fund_wallet(test_get_address(&mut rcv_wallet));

        test_create_utxos(&mut wallet, online, true, Some(1), None, FEE_RATE, None);
        test_create_utxos(
            &mut rcv_wallet,
            rcv_online,
            true,
            Some(1),
            None,
            FEE_RATE,
            None,
        );
        // issue
        let asset = test_issue_asset_nia(&mut wallet, online, None);
        // send
        let receive_data = test_blind_receive(&mut rcv_wallet);
        let recipient_map = HashMap::from([(
            asset.asset_id.clone(),
            vec![Recipient {
                assignment: Assignment::Fungible(amount),
                recipient_id: receive_data.recipient_id.clone(),
                witness_data: None,
                transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
            }],
        )]);
        let txid = test_send(&mut wallet, online, &recipient_map);
        assert!(!txid.is_empty());

        // - wait counterparty
        // UTXO 1 (input) locked, new UTXO created for change (exists = false)
        show_unspent_colorings(&mut wallet, "sender after send - WaitingCounterparty");
        test_create_utxos(&mut wallet, online, true, Some(1), None, FEE_RATE, None);
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&mut rcv_wallet, "receiver after send - WaitingCounterparty");
        let result = rcv_wallet.create_utxos(rcv_online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - wait confirmations
        wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
        wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
        // UTXO 1 now spent, UTXO 2 (RGB+BTC change) has at least 1 free allocation, UTXO 3 is empty
        show_unspent_colorings(&mut wallet, "sender after send - WaitingConfirmations");
        let result = wallet.create_utxos(online, true, Some(2), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(
            &mut rcv_wallet,
            "receiver after send - WaitingConfirmations",
        );
        let result = rcv_wallet.create_utxos(rcv_online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - settled
        mine(false, false);
        wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
        wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
        // UTXO 1 now spent, UTXO 2 (RGB+BTC change) has at least 1 free allocation, UTXO 3 is empty
        show_unspent_colorings(&mut wallet, "sender after send - Settled");
        test_create_utxos(&mut wallet, online, true, Some(3), None, FEE_RATE, Some(1));
        // UTXO 1 (blind) has at least 1 free allocation
        show_unspent_colorings(&mut rcv_wallet, "receiver after send - Settled");
        let result = rcv_wallet.create_utxos(rcv_online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // cannot create UTXOs for an empty wallet
    let (mut wallet, online) = get_empty_wallet!();
    let result = wallet.create_utxos(online, true, None, None, FEE_RATE, false);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: n,
            available: a
        }) if n == (UTXO_SIZE as u64 * UTXO_NUM as u64) + 1000 && a == 0
    ));

    fund_wallet(test_get_address(&mut wallet));
    test_create_utxos_default(&mut wallet, online);

    // don't create UTXOs if enough allocations are already available
    let result = wallet.create_utxos(online, true, None, None, FEE_RATE, false);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

    // fee min
    let result = test_create_utxos_begin_result(&mut wallet, online, false, Some(1), None, 0);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // fee overflow
    let result =
        test_create_utxos_begin_result(&mut wallet, online, false, Some(1), None, u64::MAX);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));

    // invalid amount
    let result =
        test_create_utxos_begin_result(&mut wallet, online, false, Some(1), Some(0), FEE_RATE);
    assert!(matches!(result, Err(Error::InvalidAmountZero)));

    // output below dust limit
    let result =
        test_create_utxos_begin_result(&mut wallet, online, false, Some(1), Some(1), FEE_RATE);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));
}

// create UTXOs of size funds/256 + check UTXO_NUM are created
// if casting to u8 is done improperly, this would result in trying to create 0 UTXOs
//
// see https://github.com/RGB-Tools/rgb-lib/issues/35 for context
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn casting() {
    initialize();

    let funds = 100_000_000;
    let utxo_size = funds as u32 / 256;

    let (mut wallet, online) = get_empty_wallet!();
    fund_wallet(test_get_address(&mut wallet));
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: funds,
            future: funds,
            spendable: funds,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    wait_for_btc_balance(&mut wallet, online, &expected_balance);
    test_create_utxos(
        &mut wallet,
        online,
        true,
        None,
        Some(utxo_size),
        FEE_RATE,
        None,
    );
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    // prepare wallet with 1 bitcoin UTXO
    let (mut wallet, online) = get_funded_noutxo_wallet!();

    // bitcoin UTXO not yet visible > creating UTXOs skipping sync fails
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 0);
    let result = wallet.create_utxos(online, true, None, None, FEE_RATE, true);
    assert_matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    );

    // sync so the bitcoin UTXO becomes visible
    wallet.sync(online).unwrap();
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 1);

    // create UTXOs skipping sync (returns 0 created UTXOs)
    let num_utxos_created = wallet
        .create_utxos(online, true, None, None, FEE_RATE, true)
        .unwrap();
    assert_eq!(num_utxos_created, 0);

    // created UTXOs not yet visible
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), 1);

    // created UTXOs become visible after syncing
    wallet.sync(online).unwrap();
    let unspents = test_list_unspents(&mut wallet, None, false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_reservation_interactions() {
    initialize();

    // two independent funding UTXOs, each with enough BTC to cover create_utxos
    let (mut wallet, online) = get_empty_wallet!();
    fund_wallet(test_get_address(&mut wallet));
    fund_wallet(test_get_address(&mut wallet));
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // create_utxos_begin(dry_run=false) creates CreateUtxos reservation rows
    let unsigned_psbt_str = wallet
        .create_utxos_begin(online, true, None, None, FEE_RATE, false, false)
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();
    let psbt_inputs: HashSet<(String, u32)> = unsigned_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert!(!psbt_inputs.is_empty());

    // wallet_transaction(CreateUtxos) row exists with reservations matching inputs
    let (wt, reservations) = wallet
        .database()
        .get_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
        .unwrap()
        .expect("wallet_transaction should exist after dry_run=false begin");
    assert_eq!(wt.r#type, WalletTransactionType::CreateUtxos);
    assert!(reservations.iter().all(|r| r.reserved_for == Some(wt.idx)));
    let reserved_set: HashSet<(String, u32)> = reservations
        .iter()
        .map(|r| (r.txid.clone(), r.vout))
        .collect();
    assert_eq!(reserved_set, psbt_inputs);

    // list_pending_vanilla_txs reports the in-flight CreateUtxos entry
    let pending = wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, psbt_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::CreateUtxos);
    // clear this in-flight create_utxos reservation
    wallet.abort_pending_vanilla_tx(psbt_txid).unwrap();
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());

    // reserve (at least) one vanilla UTXO via send_btc_begin(dry_run=false). BDK's
    // coin selection only picks the minimum inputs for amount + fee, so one of the
    // two funding UTXOs will be left untouched.
    let _ = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
            1000,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let reserved_set: HashSet<(String, u32)> = wallet
        .database()
        .iter_reserved_txos()
        .unwrap()
        .iter()
        .map(|r| (r.txid.clone(), r.vout))
        .collect();
    assert_eq!(reserved_set.len(), 1);

    // create_utxos_begin(dry_run=true) must not select any reserved outpoint
    let unsigned_psbt_str = wallet
        .create_utxos_begin(online, true, Some(1), None, FEE_RATE, false, true)
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let create_inputs: HashSet<(String, u32)> = unsigned_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert_eq!(create_inputs.len(), 1);
    assert!(create_inputs.is_disjoint(&reserved_set));

    // reserve the remaining vanilla UTXO via create_utxos_begin(dry_run=false)
    let _ = wallet
        .create_utxos_begin(online, true, None, None, FEE_RATE, false, false)
        .unwrap();

    // now all vanilla UTXOs are reserved, so another begin has nothing usable left
    let result = wallet.create_utxos_begin(online, false, Some(1), None, FEE_RATE, true, true);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: 0
        })
    ));
}
