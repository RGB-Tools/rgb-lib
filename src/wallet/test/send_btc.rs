use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let mut party = get_empty_party!();
    let mut rcv_party = get_empty_party!();

    // initial balance
    fund_wallet(party.get_address());
    party.create_utxos_default();
    mine(false);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 99994347,
            future: 99994347,
            spendable: 99994347,
        },
        colored: Balance {
            settled: 5000,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(party.get_btc_balance_with_sync(), expected_balance);

    // balance after send
    let bak_info_before = party.db_backup_info();
    let txid = party.send_btc(&rcv_party.get_address(), amount);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(!txid.is_empty());
    mine(false);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 99993038,
            future: 99993038,
            spendable: 99993038,
        },
        colored: Balance {
            settled: 5000,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(party.get_btc_balance_with_sync(), expected_balance);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 1000,
            future: 1000,
            spendable: 1000,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    assert_eq!(rcv_party.get_btc_balance_with_sync(), expected_balance);
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
    let result = offline_party
        .wallet
        .send_btc(Online { id: 0 }, s!(""), 0, FEE_RATE, false);
    assert_matches!(result, Err(Error::Offline));
    let result =
        offline_party
            .wallet
            .send_btc_begin(Online { id: 0 }, s!(""), 0, FEE_RATE, false, false);
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.send_btc_end(Online { id: 0 }, s!(""));
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let amount: u64 = 1000;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();
    let mut testnet_rcv_party = offline_party!(get_test_wallet_with_net(
        true,
        Some(MAX_ALLOCATIONS_PER_UTXO),
        BitcoinNetwork::Testnet,
    ));

    // bad online
    let wrong_online = Online { id: 1 };
    let good_online = party.online;
    party.online = wrong_online;
    let result = party.send_btc_result(&rcv_party.get_address(), amount);
    party.online = good_online;
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid address
    let result = party.send_btc_result("invalid", amount);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));
    let result = party.send_btc_result(&testnet_rcv_party.get_address(), amount);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // invalid amount
    let result = party.send_btc_result(&rcv_party.get_address(), 0);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // invalid fee rate (low)
    let result = party.wallet.send_btc_begin(
        party.online,
        rcv_party.get_address(),
        amount,
        0,
        false,
        true,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // invalid fee rate (overflow)
    let result = party.wallet.send_btc_begin(
        party.online,
        rcv_party.get_address(),
        amount,
        u64::MAX,
        false,
        true,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let mut party = get_empty_party!();
    let mut rcv_party = get_empty_party!();

    // prepare 1 UTXO
    fund_wallet(party.get_address());
    let unspents = party.list_unspents_with_sync(false);
    assert_eq!(unspents.len(), 1);

    // send a 1st time skipping sync (spending the only UTXO)
    let txid_1 = party
        .wallet
        .send_btc(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid_1.is_empty());
    // send a 2nd time skipping sync > succeeds because the change UTXO from send 1
    // is staged into BDK by broadcast_psbt's apply_unconfirmed_txs
    let txid_2 = party
        .wallet
        .send_btc(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid_2.is_empty());
    assert_ne!(txid_1, txid_2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_reservation_interactions() {
    initialize();

    let amount: u64 = 1000;

    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // no reservations and no SendBtc wallet_transactions initially
    assert!(party.db_reserved_txos().is_empty());
    assert!(
        party
            .db_wallet_transactions()
            .iter()
            .all(|wt| wt.r#type != WalletTransactionType::SendBtc)
    );

    // capture vanilla spendable balance before reservation
    let balance_before = party.get_btc_balance_with_sync();
    assert!(balance_before.vanilla.spendable > 0);

    // begin with dry_run=false reserves the selected inputs
    let unsigned_psbt_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            false,
            false,
        )
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();
    assert_eq!(unsigned_psbt.unsigned_tx.input.len(), 1);

    // vanilla spendable balance reflects the reservation
    let balance_reserved = party.get_btc_balance_with_sync();
    assert!(balance_reserved.vanilla.spendable < balance_before.vanilla.spendable);

    // wallet_transaction(SendBtc) row for this txid exists
    let (wt, reservations) = party
        .db_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
        .expect("should exist after begin");
    assert_eq!(wt.r#type, WalletTransactionType::SendBtc);
    // one reserved_txo per PSBT input, all pointing to that wt row
    assert!(reservations.iter().all(|r| r.reserved_for == Some(wt.idx)));
    // reservation set exactly matches PSBT inputs
    let reserved_set: HashSet<(String, u32)> = reservations
        .iter()
        .map(|r| (r.txid.clone(), r.vout))
        .collect();
    let input_set: HashSet<(String, u32)> = unsigned_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert_eq!(reserved_set, input_set);

    // list_pending_vanilla_txs reports the in-flight reservation
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, psbt_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::SendBtc);

    // sign + end releases the reservations but keeps the wallet_transaction row
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt_str, None).unwrap();
    let _end_txid = party
        .wallet
        .send_btc_end(party.online, signed_psbt)
        .unwrap();
    assert!(party.db_reserved_txos().is_empty());
    assert!(party.wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // after end, balance is no longer reduced by reservations (the UTXO is now spent,
    // and the change output is the new spendable balance)
    let balance_after_end = party.get_btc_balance_with_sync();
    assert!(balance_after_end.vanilla.spendable > balance_reserved.vanilla.spendable);
    // the wallet_transaction row is still there (so list_transactions classifies the tx)
    let wts: Vec<_> = party
        .db_wallet_transactions()
        .into_iter()
        .filter(|wt| wt.txid == psbt_txid && wt.r#type == WalletTransactionType::SendBtc)
        .collect();
    assert_eq!(wts.len(), 1);

    // list_transactions sees it as SendBtc
    mine(false);
    let transactions = party.list_transactions_with_sync();
    let entry = transactions
        .iter()
        .find(|t| t.txid == psbt_txid)
        .expect("broadcast tx should show up");
    assert!(matches!(entry.transaction_type, TransactionType::SendBtc));

    // dry_run=true begin does not create reservations/wallet_transaction row up-front
    let unsigned_psbt_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            false,
            true,
        )
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();

    // no reservations, no wallet_transaction row yet
    assert!(party.db_reserved_txos().is_empty());
    assert!(
        party
            .db_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
            .is_none()
    );
    assert!(party.wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // end still creates the SendBtc row after broadcast (for list_transactions)
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt_str, None).unwrap();
    let end_txid = party
        .wallet
        .send_btc_end(party.online, signed_psbt)
        .unwrap();
    assert_eq!(end_txid, psbt_txid);
    assert!(party.db_reserved_txos().is_empty());
    let wts: Vec<_> = party
        .db_wallet_transactions()
        .into_iter()
        .filter(|wt| wt.txid == psbt_txid && wt.r#type == WalletTransactionType::SendBtc)
        .collect();
    assert_eq!(wts.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn two_concurrent_begins_pick_disjoint_inputs() {
    initialize();

    let amount: u64 = 1000;

    // wallet with several separate vanilla UTXOs to choose from
    let mut party = get_empty_party!();
    for _ in 0..3 {
        fund_wallet(party.get_address());
    }
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

    let mut rcv_party = get_empty_party!();

    let psbt_1_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            true,
            false,
        )
        .unwrap();
    let psbt_1 = Psbt::from_str(&psbt_1_str).unwrap();
    let inputs_1: HashSet<(String, u32)> = psbt_1
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();

    let psbt_2_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            amount,
            FEE_RATE,
            true,
            false,
        )
        .unwrap();
    let psbt_2 = Psbt::from_str(&psbt_2_str).unwrap();
    let inputs_2: HashSet<(String, u32)> = psbt_2
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();

    assert!(!inputs_1.is_empty());
    assert!(!inputs_2.is_empty());
    assert!(inputs_1.is_disjoint(&inputs_2));

    // both reservations are live
    let pending = party.wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn full_send_btc_leaves_no_pending() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // a full send_btc (which uses dry_run=true internally) leaves no pending entry
    let _ = party.send_btc(&rcv_party.get_address(), 1000);
    assert!(party.wallet.list_pending_vanilla_txs().unwrap().is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_btc_end_twice() {
    initialize();

    // wallet
    let mut party = get_funded_party!();

    // prepare PSBT
    let address = party.get_address();
    let unsigned_psbt = party
        .wallet
        .send_btc_begin(party.online, address, 1000, FEE_RATE, false, false)
        .unwrap();
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();

    // call send_btc_end twice with the same PSBT, which should work (idempotent)
    party
        .wallet
        .send_btc_end(party.online, signed_psbt.clone())
        .unwrap();
    party
        .wallet
        .send_btc_end(party.online, signed_psbt)
        .unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_end() {
    initialize();

    let mut party = get_funded_noutxo_party!();
    let address = party.get_address();

    // begin does not update backup_info with dry_run=true
    let bak_info_before = party.db_backup_info();
    let _psbt = party
        .wallet
        .send_btc_begin(party.online, address.clone(), 1000, FEE_RATE, false, true)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_before.last_operation_timestamp,
        bak_info_after.last_operation_timestamp
    );

    // begin does update backup_info with dry_run=false
    let bak_info_before = party.db_backup_info();
    let psbt = party
        .wallet
        .send_btc_begin(party.online, address, 1000, FEE_RATE, false, false)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    party
        .wallet
        .send_btc_end(party.online, signed_psbt)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
