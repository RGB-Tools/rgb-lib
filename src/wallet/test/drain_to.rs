use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // receiver wallet
    let mut rcv_party = offline_party!(get_test_wallet(true, None));

    // drain funded wallet with no allocation UTXOs
    let mut party = get_funded_noutxo_party!();
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 100000000,
            future: 100000000,
            spendable: 100000000,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    party.wait_for_btc_balance(&expected_balance);
    let address = rcv_party.get_address(); // also updates backup_info
    let bak_info_before = party.db_backup_info();
    party.drain_to(&address);
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    mine(false);
    party.wait_for_unspents(false, 0);

    // issue asset (to produce an RGB allocation)
    fund_wallet(party.get_address());
    party.create_utxos_default();
    mine(false);
    party.issue_asset_nia(None);

    // drain funded wallet with RGB allocations
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
    party.wait_for_btc_balance(&expected_balance);
    party.drain_to(&rcv_party.get_address());
    mine(false);
    party.wait_for_unspents(false, 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_witness_receive() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();
    let mut drain_party = get_empty_party!();

    // issue
    let asset = party.issue_asset_nia(None);

    // send
    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // refresh receiver (no UTXOs created) + sender (to broadcast) + mine
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset.asset_id));
    mine(false);

    // receiver sees the new UTXO
    let unspents = rcv_party.list_unspents(false);
    assert_eq!(unspents.len(), 7);
    assert_eq!(unspents.iter().filter(|u| !u.utxo.exists).count(), 1);

    // drain receiver, which syncs the wallet, detecting (and draining) the new UTXO as well
    let address = drain_party.get_address();
    rcv_party.drain_to(&address);
    let unspents = rcv_party.list_unspents(false);
    assert_eq!(unspents.len(), 0);

    // refresh receiver, if draining hadn't synced (before draining) a new UTXO would appear
    rcv_party.wait_for_refresh(None);
    let unspents = rcv_party.list_unspents(false);
    assert_eq!(unspents.len(), 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn drain_to_begin_and_end_success() {
    initialize();

    // wallets
    let mut party = get_funded_noutxo_party!();
    let mut rcv_party = offline_party!(get_test_wallet(true, None));

    let address = rcv_party.get_address();
    let bak_info_before = party.db_backup_info();

    // drain_to_begin does not update backup_info
    let unsigned_psbt = party
        .wallet
        .drain_to_begin(party.online, address, FEE_RATE, true)
        .unwrap();
    let bak_info_after_begin = party.db_backup_info();
    assert_eq!(
        bak_info_after_begin.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // sign and broadcast via drain_to_end
    let signed_psbt = party.wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let txid = party
        .wallet
        .drain_to_end(party.online, signed_psbt)
        .unwrap();
    assert!(!txid.is_empty());

    // drain_to_end updates backup_info
    let bak_info_after_end = party.db_backup_info();
    assert!(bak_info_after_end.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // verify the drain was effective
    mine(false);
    party.wait_for_unspents(false, 0);
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
        .drain_to(Online { id: 0 }, s!(""), FEE_RATE);
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party
        .wallet
        .drain_to_begin(Online { id: 0 }, s!(""), FEE_RATE, false);
    assert_matches!(result, Err(Error::Offline));
    let result = offline_party.wallet.drain_to_end(Online { id: 0 }, s!(""));
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    // wallets
    let mut party = get_empty_party!();
    let mut rcv_party = get_empty_party!();

    // drain empty wallet
    let result = party.drain_to_result(&rcv_party.get_address());
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // bad online object
    fund_wallet(party.get_address());
    let good_online = party.online;
    party.online = rcv_party.online;
    let result = party.drain_to_result(&rcv_party.get_address());
    party.online = good_online;
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // bad address
    let result = party.drain_to_result("invalid address");
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // fee min
    fund_wallet(party.get_address());
    let result = party.drain_to_begin_result(&rcv_party.get_address(), 0);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // fee overflow
    fund_wallet(party.get_address());
    let result = party.drain_to_begin_result(&rcv_party.get_address(), u64::MAX);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));

    // no private keys
    let mut wo_party = get_funded_noutxo_party(false, None);
    let result = wo_party.drain_to_result(&rcv_party.get_address());
    assert!(matches!(result, Err(Error::WatchOnly)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reservation_interaction() {
    initialize();

    // wallet with several vanilla UTXOs so they can be reserved
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
    let mut drain_party = get_empty_party!();

    // reserve all vanilla UTXO via drain_to_begin(dry_run=false)
    let psbt = party
        .wallet
        .drain_to_begin(party.online, rcv_party.get_address(), FEE_RATE, false)
        .unwrap();

    // check send_btc cannot spend the reserved UTXOs
    let res = party.wallet.send_btc_begin(
        party.online,
        rcv_party.get_address(),
        1000,
        FEE_RATE,
        true,
        false,
    );
    assert_matches!(
        res,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    );

    // cancel pending drain_to to unlock the reserved inputs
    let txid = Psbt::from_str(&psbt).unwrap().get_txid().to_string();
    party.wallet.abort_pending_vanilla_tx(txid).unwrap();

    // reserve one (or more) vanilla UTXO via send_btc_begin(dry_run=false)
    let send_psbt_str = party
        .wallet
        .send_btc_begin(
            party.online,
            rcv_party.get_address(),
            1000,
            FEE_RATE,
            true,
            false,
        )
        .unwrap();
    let send_psbt = Psbt::from_str(&send_psbt_str).unwrap();
    let reserved_inputs: HashSet<(String, u32)> = send_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert!(!reserved_inputs.is_empty());

    // drain always spends all wallet UTXOs, including those reserved by in-flight vanilla
    // transactions
    let drain_psbt_str = party
        .wallet
        .drain_to_begin(party.online, drain_party.get_address(), FEE_RATE, true)
        .unwrap();
    let drain_psbt = Psbt::from_str(&drain_psbt_str).unwrap();
    let drain_inputs: HashSet<(String, u32)> = drain_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert!(!drain_inputs.is_empty());
    assert!(
        reserved_inputs
            .iter()
            .all(|outpoint| drain_inputs.contains(outpoint))
    );
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
        .drain_to_begin(party.online, address.clone(), FEE_RATE, true)
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
        .drain_to_begin(party.online, address, FEE_RATE, false)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    let signed_psbt = party.wallet.sign_psbt(psbt, None).unwrap();

    // end updates backup_info
    let bak_info_before = party.db_backup_info();
    party
        .wallet
        .drain_to_end(party.online, signed_psbt)
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
}
