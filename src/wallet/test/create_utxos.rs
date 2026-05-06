use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // up_to version with 0 allocatable UTXOs
    println!("\n=== up_to true, 0 allocatable");
    let mut party = get_funded_noutxo_party!();
    let bak_info_before = party.db_backup_info();
    party.create_utxos(true, None, None, FEE_RATE, None);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // up_to version with allocatable UTXOs partially available (1 missing)
    println!("\n=== up_to true, need to create 1 more");
    party.create_utxos(true, Some(UTXO_NUM + 1), None, FEE_RATE, Some(1));
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM + 2) as usize);

    // forced version always creates UTXOs
    println!("\n=== up_to false");
    party.create_utxos(false, None, None, FEE_RATE, None);
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM * 2 + 2) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn up_to_allocation_checks() {
    initialize();

    let amount = 66;

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = get_empty_party!();

    // MAX_ALLOCATIONS_PER_UTXO failed allocations
    //  - check unspent counted as allocatable
    party.create_utxos(false, Some(1), None, FEE_RATE, None);
    let mut batch_transfer_idxs: Vec<i32> = vec![];
    let mut txo_list: HashSet<Outpoint> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = party.blind_receive();
        let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
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
        assert!(party.fail_transfers_single(batch_transfer_idx));
    }
    // request 1 new UTXO, expecting the existing one is still allocatable
    let result = party
        .wallet
        .create_utxos(party.online, true, Some(1), None, FEE_RATE, false);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 2);

    // new wallet
    let mut party = get_funded_noutxo_party!();

    // MAX_ALLOCATIONS_PER_UTXO allocations
    party.create_utxos(true, Some(1), None, FEE_RATE, None);
    // create MAX_ALLOCATIONS_PER_UTXO blinds on the same UTXO
    let mut txo_list: HashSet<Outpoint> = HashSet::new();
    for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
        let receive_data = party.blind_receive();
        let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
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
    party.create_utxos(true, Some(1), None, FEE_RATE, None);
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 3);

    if MAX_ALLOCATIONS_PER_UTXO > 2 {
        // new wallet
        let mut party = get_funded_noutxo_party!();
        fund_wallet(rcv_party.get_address());

        party.create_utxos(true, Some(1), None, FEE_RATE, None);
        rcv_party.create_utxos(true, Some(1), None, FEE_RATE, None);
        // issue
        let asset = party.issue_asset_nia(None);
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
        let txid = party.send_retry(&recipient_map);
        assert!(!txid.is_empty());

        // - wait counterparty
        // UTXO 1 (input) locked, new UTXO created for change (exists = false)
        party.show_unspent_colorings("sender after send - WaitingCounterparty");
        party.create_utxos(true, Some(1), None, FEE_RATE, None);
        // UTXO 1 (blind) has at least 1 free allocation
        rcv_party.show_unspent_colorings("receiver after send - WaitingCounterparty");
        let result =
            rcv_party
                .wallet
                .create_utxos(rcv_party.online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - wait confirmations
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(Some(&asset.asset_id));
        // UTXO 1 now spent, UTXO 2 (RGB+BTC change) has at least 1 free allocation, UTXO 3 is empty
        party.show_unspent_colorings("sender after send - WaitingConfirmations");
        let result = party
            .wallet
            .create_utxos(party.online, true, Some(2), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
        // UTXO 1 (blind) has at least 1 free allocation
        rcv_party.show_unspent_colorings("receiver after send - WaitingConfirmations");
        let result =
            rcv_party
                .wallet
                .create_utxos(rcv_party.online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

        // - settled
        mine(false);
        rcv_party.wait_for_refresh(None);
        party.wait_for_refresh(Some(&asset.asset_id));
        // UTXO 1 now spent, UTXO 2 (RGB+BTC change) has at least 1 free allocation, UTXO 3 is empty
        party.show_unspent_colorings("sender after send - Settled");
        party.create_utxos(true, Some(3), None, FEE_RATE, Some(1));
        // UTXO 1 (blind) has at least 1 free allocation
        rcv_party.show_unspent_colorings("receiver after send - Settled");
        let result =
            rcv_party
                .wallet
                .create_utxos(rcv_party.online, true, Some(1), None, FEE_RATE, false);
        assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));
    }
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // cannot create UTXOs for an empty wallet
    let mut party = get_empty_party!();
    let result = party
        .wallet
        .create_utxos(party.online, true, None, None, FEE_RATE, false);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: n,
            available: a
        }) if n == (UTXO_SIZE as u64 * UTXO_NUM as u64) + 1000 && a == 0
    ));

    fund_wallet(party.get_address());
    party.create_utxos_default();

    // don't create UTXOs if enough allocations are already available
    let result = party
        .wallet
        .create_utxos(party.online, true, None, None, FEE_RATE, false);
    assert!(matches!(result, Err(Error::AllocationsAlreadyAvailable)));

    // fee min
    let result = party.create_utxos_begin_result(false, Some(1), None, 0);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // fee overflow
    let result = party.create_utxos_begin_result(false, Some(1), None, u64::MAX);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));

    // invalid amount
    let result = party.create_utxos_begin_result(false, Some(1), Some(0), FEE_RATE);
    assert!(matches!(result, Err(Error::InvalidAmountZero)));

    // output below dust limit
    let result = party.create_utxos_begin_result(false, Some(1), Some(1), FEE_RATE);
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

    let mut party = get_empty_party!();
    fund_wallet(party.get_address());
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
    party.wait_for_btc_balance(&expected_balance);
    party.create_utxos(true, None, Some(utxo_size), FEE_RATE, None);
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    // prepare wallet with 1 bitcoin UTXO
    let mut party = get_funded_noutxo_party!();

    // bitcoin UTXO not yet visible > creating UTXOs skipping sync fails
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 0);
    let result = party
        .wallet
        .create_utxos(party.online, true, None, None, FEE_RATE, true);
    assert_matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    );

    // sync so the bitcoin UTXO becomes visible
    party
        .wallet
        .sync(
            party.online,
            SyncOptions {
                keychain: SyncKeychain::Vanilla {
                    lookback: INDEXER_SYNC_LOOKBACK as u32,
                },
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), 1);

    // create UTXOs skipping sync
    let num_utxos_created = party
        .wallet
        .create_utxos(party.online, true, None, None, FEE_RATE, true)
        .unwrap();
    assert_eq!(num_utxos_created, UTXO_NUM);

    // created UTXOs already visible
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);

    // created UTXOs still consistent after syncing
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
    let unspents = party.list_unspents(false);
    assert_eq!(unspents.len(), (UTXO_NUM + 1) as usize);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_reservation_interactions() {
    initialize();

    // two independent funding UTXOs, each with enough BTC to cover create_utxos
    let mut party = get_empty_party!();
    fund_wallet(party.get_address());
    fund_wallet(party.get_address());
    let mut rcv_party = get_empty_party!();

    // create_utxos_begin(dry_run=false) creates CreateUtxos reservation rows
    let unsigned_psbt_str = party
        .wallet
        .create_utxos_begin(party.online, true, None, None, FEE_RATE, false, false)
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
    let (wt, reservations) = party
        .db_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
        .expect("wallet_transaction should exist after dry_run=false begin");
    assert_eq!(wt.r#type, WalletTransactionType::CreateUtxos);
    assert!(reservations.iter().all(|r| r.reserved_for == Some(wt.idx)));
    let reserved_set: HashSet<(String, u32)> = reservations
        .iter()
        .map(|r| (r.txid.clone(), r.vout))
        .collect();
    assert_eq!(reserved_set, psbt_inputs);

    // list_pending_vanilla_txs reports the in-flight CreateUtxos entry
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, psbt_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::CreateUtxos);
    // clear this in-flight create_utxos reservation
    party.wallet.abort_pending_vanilla_tx(psbt_txid).unwrap();
    assert!(party.db_reserved_txos().is_empty());

    // reserve (at least) one vanilla UTXO via send_btc_begin(dry_run=false). BDK's
    // coin selection only picks the minimum inputs for amount + fee, so one of the
    // two funding UTXOs will be left untouched.
    let _ = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            1000,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let reserved_set: HashSet<(String, u32)> = party
        .db_reserved_txos()
        .iter()
        .map(|r| (r.txid.clone(), r.vout))
        .collect();
    assert_eq!(reserved_set.len(), 1);

    // create_utxos_begin(dry_run=true) must not select any reserved outpoint
    let unsigned_psbt_str = party
        .wallet
        .create_utxos_begin(party.online, true, Some(1), None, FEE_RATE, false, true)
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
    let _ = party
        .wallet
        .create_utxos_begin(party.online, true, None, None, FEE_RATE, false, false)
        .unwrap();

    // now all vanilla UTXOs are reserved, so another begin has nothing usable left
    let result =
        party
            .wallet
            .create_utxos_begin(party.online, false, Some(1), None, FEE_RATE, true, true);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: 0
        })
    ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let mut party = get_funded_noutxo_party!();

    // begin does not update backup_info with dry_run=true
    let bak_info_before = party.db_backup_info();
    let _psbt = party
        .wallet
        .create_utxos_begin(party.online, true, None, None, FEE_RATE, false, true)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let bak_info_before = party.db_backup_info();
    let psbt = party
        .wallet
        .create_utxos_begin(party.online, true, None, None, FEE_RATE, false, false)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    party
        .wallet
        .create_utxos_end(party.online, signed_psbt)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
