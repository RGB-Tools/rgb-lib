use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // initial balance
    fund_wallet(test_get_address(&mut wallet));
    test_create_utxos_default(&mut wallet, online);
    mine(false, false);
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
    assert_eq!(test_get_btc_balance(&mut wallet, online), expected_balance);

    // balance after send
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();
    let txid = test_send_btc(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        amount,
    );
    let bak_info_after = wallet.database().get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert!(!txid.is_empty());
    mine(false, false);
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
    assert_eq!(test_get_btc_balance(&mut wallet, online), expected_balance);
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
    assert_eq!(
        test_get_btc_balance(&mut rcv_wallet, rcv_online),
        expected_balance
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();
    let mut testnet_rcv_wallet = get_test_wallet_with_net(
        true,
        Some(MAX_ALLOCATIONS_PER_UTXO),
        BitcoinNetwork::Testnet,
    );

    // bad online
    let wrong_online = Online { id: 1 };
    let result = test_send_btc_result(
        &mut wallet,
        wrong_online,
        &test_get_address(&mut rcv_wallet),
        amount,
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid address
    let result = test_send_btc_result(&mut wallet, online, "invalid", amount);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));
    let result = test_send_btc_result(
        &mut wallet,
        online,
        &test_get_address(&mut testnet_rcv_wallet),
        amount,
    );
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // invalid amount
    let result = test_send_btc_result(&mut wallet, online, &test_get_address(&mut rcv_wallet), 0);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // invalid fee rate (low)
    let result = wallet.send_btc_begin(
        online,
        test_get_address(&mut rcv_wallet),
        amount,
        0,
        false,
        true,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // invalid fee rate (overflow)
    let result = wallet.send_btc_begin(
        online,
        test_get_address(&mut rcv_wallet),
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
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // prepare 1 UTXO
    fund_wallet(test_get_address(&mut wallet));
    let unspents = test_list_unspents(&mut wallet, Some(online), false);
    assert_eq!(unspents.len(), 1);

    // send a 1st time skipping sync (spending the only UTXO)
    let txid = wallet
        .send_btc(
            online,
            test_get_address(&mut rcv_wallet),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid.is_empty());
    // send a 2nd time skipping sync > FailedBroadcast
    let result = wallet.send_btc(
        online,
        test_get_address(&mut rcv_wallet),
        amount,
        FEE_RATE,
        true,
    );
    assert_matches!(result, Err(Error::FailedBroadcast { details: _ }));

    // sync and retry the 2nd send skipping sync > change UTXO is now available
    wallet.sync(online).unwrap();
    let txid = wallet
        .send_btc(
            online,
            test_get_address(&mut rcv_wallet),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid.is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn begin_reservation_interactions() {
    initialize();

    let amount: u64 = 1000;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // no reservations and no SendBtc wallet_transactions initially
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());
    assert!(
        wallet
            .database()
            .iter_wallet_transactions()
            .unwrap()
            .iter()
            .all(|wt| wt.r#type != WalletTransactionType::SendBtc)
    );

    // capture vanilla spendable balance before reservation
    let balance_before = test_get_btc_balance(&mut wallet, online);
    assert!(balance_before.vanilla.spendable > 0);

    // begin with dry_run=false reserves the selected inputs
    let unsigned_psbt_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
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
    let balance_reserved = test_get_btc_balance(&mut wallet, online);
    assert!(balance_reserved.vanilla.spendable < balance_before.vanilla.spendable);

    // wallet_transaction(SendBtc) row for this txid exists
    let (wt, reservations) = wallet
        .database()
        .get_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
        .unwrap()
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
    let pending = wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].txid, psbt_txid);
    assert_eq!(pending[0].r#type, WalletTransactionType::SendBtc);

    // sign + end releases the reservations but keeps the wallet_transaction row
    let signed_psbt = wallet.sign_psbt(unsigned_psbt_str, None).unwrap();
    let _end_txid = wallet.send_btc_end(online, signed_psbt, false).unwrap();
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());
    assert!(wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // after end, balance is no longer reduced by reservations (the UTXO is now spent,
    // and the change output is the new spendable balance)
    let balance_after_end = test_get_btc_balance(&mut wallet, online);
    assert!(balance_after_end.vanilla.spendable > balance_reserved.vanilla.spendable);
    // the wallet_transaction row is still there (so list_transactions classifies the tx)
    let wts: Vec<_> = wallet
        .database()
        .iter_wallet_transactions()
        .unwrap()
        .into_iter()
        .filter(|wt| wt.txid == psbt_txid && wt.r#type == WalletTransactionType::SendBtc)
        .collect();
    assert_eq!(wts.len(), 1);

    // list_transactions sees it as SendBtc
    mine(false, false);
    let transactions = test_list_transactions(&mut wallet, Some(online));
    let entry = transactions
        .iter()
        .find(|t| t.txid == psbt_txid)
        .expect("broadcast tx should show up");
    assert!(matches!(entry.transaction_type, TransactionType::SendBtc));

    // dry_run=true begin does not create reservations/wallet_transaction row up-front
    let unsigned_psbt_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
            amount,
            FEE_RATE,
            false,
            true,
        )
        .unwrap();
    let unsigned_psbt = Psbt::from_str(&unsigned_psbt_str).unwrap();
    let psbt_txid = unsigned_psbt.unsigned_tx.compute_txid().to_string();

    // no reservations, no wallet_transaction row yet
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());
    assert!(
        wallet
            .database()
            .get_wallet_transaction_with_reserved_txos_by_txid(&psbt_txid)
            .unwrap()
            .is_none()
    );
    assert!(wallet.list_pending_vanilla_txs().unwrap().is_empty());

    // end still creates the SendBtc row after broadcast (for list_transactions)
    let signed_psbt = wallet.sign_psbt(unsigned_psbt_str, None).unwrap();
    let end_txid = wallet.send_btc_end(online, signed_psbt, false).unwrap();
    assert_eq!(end_txid, psbt_txid);
    assert!(wallet.database().iter_reserved_txos().unwrap().is_empty());
    let wts: Vec<_> = wallet
        .database()
        .iter_wallet_transactions()
        .unwrap()
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
    let (mut wallet, online) = get_empty_wallet!();
    for _ in 0..3 {
        fund_wallet(test_get_address(&mut wallet));
    }
    wallet.sync(online).unwrap();

    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    let psbt_1_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
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

    let psbt_2_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
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
    let pending = wallet.list_pending_vanilla_txs().unwrap();
    assert_eq!(pending.len(), 2);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn full_send_btc_leaves_no_pending() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // a full send_btc (which uses dry_run=true internally) leaves no pending entry
    let _ = test_send_btc(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        1000,
    );
    assert!(wallet.list_pending_vanilla_txs().unwrap().is_empty());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn send_btc_end_twice() {
    initialize();

    // wallet
    let (mut wallet, online) = get_funded_wallet!();

    // prepare PSBT
    let address = test_get_address(&mut wallet);
    let unsigned_psbt = wallet.send_btc_begin(online, address, 1000, FEE_RATE, false, false).unwrap();
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();

    // call send_btc_end twice with the same PSBT, which should work (idempotent)
    wallet.send_btc_end(online, signed_psbt.clone(), false).unwrap();
    wallet.send_btc_end(online, signed_psbt, false).unwrap();
}
