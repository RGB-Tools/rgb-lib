use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // receiver wallet
    let mut rcv_wallet = get_test_wallet(true, None);

    // drain funded wallet with no allocation UTXOs
    let (mut wallet, online) = get_funded_noutxo_wallet!();
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
    wait_for_btc_balance(&mut wallet, online, &expected_balance);
    let address = test_get_address(&mut rcv_wallet); // also updates backup_info
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();
    test_drain_to_keep(&mut wallet, online, &address);
    let bak_info_after = wallet.database().get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    mine(false, false);
    wait_for_unspents(&mut wallet, Some(online), false, 0);

    // issue asset (to produce an RGB allocation)
    fund_wallet(test_get_address(&mut wallet));
    test_create_utxos_default(&mut wallet, online);
    mine(false, false);
    test_issue_asset_nia(&mut wallet, online, None);

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
    wait_for_btc_balance(&mut wallet, online, &expected_balance);
    test_drain_to_keep(&mut wallet, online, &test_get_address(&mut rcv_wallet));
    mine(false, false);
    wait_for_unspents(&mut wallet, Some(online), false, UTXO_NUM);
    test_drain_to_destroy(&mut wallet, online, &test_get_address(&mut rcv_wallet));
    mine(false, false);
    wait_for_unspents(&mut wallet, Some(online), false, 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_witness_receive() {
    initialize();

    let amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();
    let (mut drain_wallet, _drain_online) = get_empty_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, online, None);

    // send
    let receive_data = test_witness_receive(&mut rcv_wallet);
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
    let txid = test_send(&mut wallet, online, &recipient_map);
    assert!(!txid.is_empty());

    // refresh receiver (no UTXOs created) + sender (to broadcast) + mine
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    wait_for_refresh(&mut wallet, online, Some(&asset.asset_id), None);
    mine(false, false);

    // receiver sees the new UTXO
    let unspents = list_test_unspents(&mut rcv_wallet, "before draining");
    assert_eq!(unspents.len(), 7);
    assert_eq!(unspents.iter().filter(|u| !u.utxo.exists).count(), 1);

    // drain receiver, which syncs the wallet, detecting (and draining) the new UTXO as well
    let address = test_get_address(&mut drain_wallet);
    test_drain_to_destroy(&mut rcv_wallet, rcv_online, &address);
    let unspents = list_test_unspents(&mut rcv_wallet, "after draining");
    assert_eq!(unspents.len(), 0);

    // refresh receiver, if draining hadn't synced (before draining) a new UTXO would appear
    wait_for_refresh(&mut rcv_wallet, rcv_online, None, None);
    let unspents = list_test_unspents(&mut rcv_wallet, "after receiver refresh 2");
    assert_eq!(unspents.len(), 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn drain_to_begin_and_end_success() {
    initialize();

    // wallets
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let mut rcv_wallet = get_test_wallet(true, None);

    let address = test_get_address(&mut rcv_wallet);
    let bak_info_before = wallet.database().get_backup_info().unwrap().unwrap();

    // drain_to_begin does not update backup_info
    let unsigned_psbt = wallet
        .drain_to_begin(online, address, false, FEE_RATE, true)
        .unwrap();
    let bak_info_after_begin = wallet.database().get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after_begin.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    // sign and broadcast via drain_to_end
    let signed_psbt = wallet.sign_psbt(unsigned_psbt, None).unwrap();
    let txid = wallet.drain_to_end(online, signed_psbt).unwrap();
    assert!(!txid.is_empty());

    // drain_to_end updates backup_info
    let bak_info_after_end = wallet.database().get_backup_info().unwrap().unwrap();
    assert!(bak_info_after_end.last_operation_timestamp > bak_info_before.last_operation_timestamp);

    // verify the drain was effective
    mine(false, false);
    wait_for_unspents(&mut wallet, Some(online), false, 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let (mut wallet, online) = get_empty_wallet!();
    let (mut rcv_wallet, rcv_online) = get_empty_wallet!();

    // drain empty wallet
    let result = test_drain_to_result(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        true,
    );
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // bad online object
    fund_wallet(test_get_address(&mut wallet));
    let result = test_drain_to_result(
        &mut wallet,
        rcv_online,
        &test_get_address(&mut rcv_wallet),
        false,
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // bad address
    let result = test_drain_to_result(&mut wallet, online, "invalid address", false);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // fee min
    fund_wallet(test_get_address(&mut wallet));
    let result = test_drain_to_begin_result(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        true,
        0,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));

    // fee overflow
    fund_wallet(test_get_address(&mut wallet));
    let result = test_drain_to_begin_result(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        true,
        u64::MAX,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_OVER));

    // no private keys
    let (mut wallet, online) = get_funded_noutxo_wallet(false, None);
    let result = test_drain_to_result(
        &mut wallet,
        online,
        &test_get_address(&mut rcv_wallet),
        false,
    );
    assert!(matches!(result, Err(Error::WatchOnly)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reservation_interaction() {
    initialize();

    // wallet with several vanilla UTXOs so drain keep can still succeed while one
    // is reserved
    let (mut wallet, online) = get_empty_wallet!();
    for _ in 0..3 {
        fund_wallet(test_get_address(&mut wallet));
    }
    wallet.sync(online).unwrap();

    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();
    let (mut drain_wallet, _drain_online) = get_empty_wallet!();

    // reserve one (or more) vanilla UTXO via send_btc_begin(dry_run=false)
    let send_psbt_str = wallet
        .send_btc_begin(
            online,
            test_get_address(&mut rcv_wallet),
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

    // drain (keep assets) must avoid the reserved outpoints
    let drain_psbt_str = wallet
        .drain_to_begin(
            online,
            test_get_address(&mut drain_wallet),
            false,
            FEE_RATE,
            true,
        )
        .unwrap();
    let drain_psbt = Psbt::from_str(&drain_psbt_str).unwrap();
    let drain_inputs: HashSet<(String, u32)> = drain_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    assert!(!drain_inputs.is_empty());
    assert!(drain_inputs.is_disjoint(&reserved_inputs));

    // drain with destroy_assets=true ignores reservations: it consumes the reserved outpoints along
    // with everything else
    let destroy_psbt_str = wallet
        .drain_to_begin(
            online,
            test_get_address(&mut drain_wallet),
            true,
            FEE_RATE,
            true,
        )
        .unwrap();
    let destroy_psbt = Psbt::from_str(&destroy_psbt_str).unwrap();
    let destroy_inputs: HashSet<(String, u32)> = destroy_psbt
        .unsigned_tx
        .input
        .iter()
        .map(|i| (i.previous_output.txid.to_string(), i.previous_output.vout))
        .collect();
    // the destroy drain should include all reserved outpoints
    assert!(
        reserved_inputs
            .iter()
            .all(|outpoint| destroy_inputs.contains(outpoint))
    );
}
