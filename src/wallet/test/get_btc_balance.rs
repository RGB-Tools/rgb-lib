use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();

    // empty balance
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let balance = test_get_btc_balance(&mut wallet, &online);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    assert_eq!(balance, expected_balance);

    // future balance after funding
    stop_mining();
    send_to_address(test_get_address(&mut wallet));
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 100000000,
            spendable: 100000000,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    wait_for_btc_balance(&mut wallet, &online, &expected_balance);

    // settled balance after mining
    mine(false, true);
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
    assert_eq!(test_get_btc_balance(&mut wallet, &online), expected_balance);

    // future vanilla change + colored UTXOs balance
    stop_mining();
    test_create_utxos_default(&mut wallet, &online);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 99994347,
            spendable: 99994347,
        },
        colored: Balance {
            settled: 0,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(test_get_btc_balance(&mut wallet, &online), expected_balance);

    // settled balance after mining
    mine(false, true);
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
    assert_eq!(test_get_btc_balance(&mut wallet, &online), expected_balance);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let check_timeout = 10;
    let check_interval = 1000;

    fn get_check<'a>(
        wallet: &'a mut Wallet,
        online: &'a Online,
        expected_balance: &'a BtcBalance,
        sync: bool,
    ) -> impl FnMut() -> bool + 'a {
        move || -> bool {
            if sync {
                wallet.sync(online.clone()).unwrap();
            }
            let balance = wallet.get_btc_balance(None, true).unwrap();
            balance == *expected_balance
        }
    }

    let (mut wallet, online) = get_empty_wallet!();

    // empty balance
    let balance = wallet.get_btc_balance(None, true).unwrap();
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    assert_eq!(balance, expected_balance);

    // future balance after funding
    stop_mining();
    send_to_address(test_get_address(&mut wallet));
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 100000000,
            spendable: 100000000,
        },
        colored: Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        },
    };
    // no change to balance if sync is skipped
    assert!(!wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // settled balance after mining
    mine(false, true);
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
    // no change to balance if sync is skipped
    assert!(!wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // future vanilla change + colored UTXOs balance (create UTXOs skipping sync)
    stop_mining();
    wallet
        .create_utxos(online.clone(), false, None, None, FEE_RATE, true)
        .unwrap();
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 99994347,
            spendable: 99994347,
        },
        colored: Balance {
            settled: 0,
            future: 5000,
            spendable: 5000,
        },
    };
    // no change to balance if sync is skipped
    assert!(!wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // settled balance after mining
    mine(false, true);
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
    // no change to balance if sync is skipped
    assert!(!wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut wallet, &online, &expected_balance, true),
        check_timeout,
        check_interval,
    ));
}
