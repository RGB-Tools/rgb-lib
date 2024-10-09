use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (wallet, online) = get_empty_wallet!();

    // empty balance
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let balance = test_get_btc_balance(&wallet, &online);
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
    send_to_address(test_get_address(&wallet));
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
    wait_for_btc_balance(&wallet, &online, &expected_balance);

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
    assert_eq!(test_get_btc_balance(&wallet, &online), expected_balance);

    // future vanilla change + colored UTXOs balance
    stop_mining();
    test_create_utxos_default(&wallet, &online);
    let expected_balance = BtcBalance {
        vanilla: Balance {
            settled: 0,
            future: 99994508,
            spendable: 99994508,
        },
        colored: Balance {
            settled: 0,
            future: 5000,
            spendable: 5000,
        },
    };
    assert_eq!(test_get_btc_balance(&wallet, &online), expected_balance);

    // settled balance after mining
    mine(false, true);
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
}
