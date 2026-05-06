use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_empty_party!();

    // empty balance
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    let balance = party.get_btc_balance_with_sync();
    let bak_info_after = party.db_backup_info_opt();
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
    let _guard = stop_mining();
    send_to_address(party.get_address());
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
    party.wait_for_btc_balance(&expected_balance);

    // settled balance after mining
    drop(_guard);
    mine(false);
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
    assert_eq!(party.get_btc_balance_with_sync(), expected_balance);

    // future vanilla change + colored UTXOs balance
    let _guard = stop_mining();
    party.create_utxos_default();
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
    assert_eq!(party.get_btc_balance_with_sync(), expected_balance);

    // settled balance after mining
    drop(_guard);
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
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn skip_sync() {
    initialize();

    let check_timeout = 10;
    let check_interval = 1000;

    fn get_check<'a>(
        party: &'a mut impl SigParty<W = Wallet>,
        expected_balance: &'a BtcBalance,
        sync: bool,
    ) -> impl FnMut() -> bool + 'a {
        move || -> bool {
            if sync {
                party.sync(SyncOptions {
                    keychain: SyncKeychain::Vanilla {
                        lookback: INDEXER_SYNC_LOOKBACK as u32,
                    },
                    strategy: SyncStrategy::FastSync,
                });
            }
            let balance = party.get_btc_balance();
            balance == *expected_balance
        }
    }

    let mut party = get_empty_party!();

    // empty balance
    let balance = party.get_btc_balance();
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
    let _guard = stop_mining();
    send_to_address(party.get_address());
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
        get_check(&mut party, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut party, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // settled balance after mining
    drop(_guard);
    mine(false);
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
        get_check(&mut party, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut party, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // future vanilla change + colored UTXOs balance (create UTXOs skipping sync)
    let _guard = stop_mining();
    party
        .wallet
        .create_utxos(party.online, false, None, None, FEE_RATE, true)
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
    // balance reflects the self-broadcast TX immediately (no manual sync needed)
    assert!(wait_for_function(
        get_check(&mut party, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // still consistent after a manual sync
    assert!(wait_for_function(
        get_check(&mut party, &expected_balance, true),
        check_timeout,
        check_interval,
    ));

    // settled balance after mining
    drop(_guard);
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
    // no change to balance if sync is skipped
    assert!(!wait_for_function(
        get_check(&mut party, &expected_balance, false),
        check_timeout,
        check_interval,
    ));
    // balance updated after manual sync
    assert!(wait_for_function(
        get_check(&mut party, &expected_balance, true),
        check_timeout,
        check_interval,
    ));
}
