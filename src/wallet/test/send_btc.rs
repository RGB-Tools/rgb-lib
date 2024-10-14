use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let (wallet, online) = get_empty_wallet!();
    let (rcv_wallet, rcv_online) = get_empty_wallet!();

    // initial balance
    fund_wallet(test_get_address(&wallet));
    test_create_utxos_default(&wallet, &online);
    mine(false, false);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 99994508,
            future: 99994508,
            spendable: 99994508,
        },
        colored: Balance {
            settled: 5000,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(test_get_btc_balance(&wallet, &online), expected_balance);

    // balance after send
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let txid = test_send_btc(&wallet, &online, &test_get_address(&rcv_wallet), amount);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert!(!txid.is_empty());
    mine(false, false);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 99993274,
            future: 99993274,
            spendable: 99993274,
        },
        colored: Balance {
            settled: 5000,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(test_get_btc_balance(&wallet, &online), expected_balance);
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
        test_get_btc_balance(&rcv_wallet, &rcv_online),
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
    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_empty_wallet!();
    let testnet_rcv_wallet = get_test_wallet_with_net(
        true,
        Some(MAX_ALLOCATIONS_PER_UTXO),
        BitcoinNetwork::Testnet,
    );

    // bad online
    let wrong_online = Online {
        id: 1,
        indexer_url: wallet.online_data.as_ref().unwrap().indexer_url.clone(),
    };
    let result = test_send_btc_result(
        &wallet,
        &wrong_online,
        &test_get_address(&rcv_wallet),
        amount,
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid address
    let result = test_send_btc_result(&wallet, &online, "invalid", amount);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));
    let result = test_send_btc_result(
        &wallet,
        &online,
        &test_get_address(&testnet_rcv_wallet),
        amount,
    );
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // invalid amount
    let result = test_send_btc_result(&wallet, &online, &test_get_address(&rcv_wallet), 0);
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // invalid fee rate
    let result = wallet.send_btc_begin(
        online.clone(),
        test_get_address(&rcv_wallet),
        amount,
        0.9,
        false,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let (wallet, online) = get_empty_wallet!();
    let (rcv_wallet, _rcv_online) = get_empty_wallet!();

    // prepare 1 UTXO
    fund_wallet(test_get_address(&wallet));
    let unspents = test_list_unspents(&wallet, Some(&online), false);
    assert_eq!(unspents.len(), 1);

    // send a 1st time skipping sync (spending the only UTXO)
    let txid = wallet
        .send_btc(
            online.clone(),
            test_get_address(&rcv_wallet),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid.is_empty());
    // send a 2nd time skipping sync > FailedBroadcast
    let result = wallet.send_btc(
        online.clone(),
        test_get_address(&rcv_wallet),
        amount,
        FEE_RATE,
        true,
    );
    assert_matches!(result, Err(Error::FailedBroadcast { details: _ }));

    // sync and retry the 2nd send skipping sync > change UTXO is now available
    wallet.sync(online.clone()).unwrap();
    let txid = wallet
        .send_btc(
            online.clone(),
            test_get_address(&rcv_wallet),
            amount,
            FEE_RATE,
            true,
        )
        .unwrap();
    assert!(!txid.is_empty());
}
