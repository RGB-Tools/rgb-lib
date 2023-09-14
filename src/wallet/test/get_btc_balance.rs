use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();

    // empty balances
    let balances = wallet.get_btc_balance(online.clone()).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));

    // future balance after funding
    stop_mining();
    fund_wallet(wallet.get_address());
    wait_for_unspent_num(&wallet, online.clone(), 1);
    let balances = wallet.get_btc_balance(online.clone()).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 0,
            future: 100000000,
            spendable: 100000000,
        }
    ));
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));

    // settled balance after mining
    mine(true);
    let balances = wallet.get_btc_balance(online.clone()).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 100000000,
            future: 100000000,
            spendable: 100000000,
        }
    ));
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));

    // future vanilla change + colored UTXOs balance
    stop_mining();
    test_create_utxos_default(&mut wallet, online.clone());
    let balances = wallet.get_btc_balance(online.clone()).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 0,
            future: 99994601,
            spendable: 99994601,
        }
    ));
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 0,
            future: 5000,
            spendable: 5000,
        }
    ));

    // settled balance after mining
    mine(true);
    let balances = wallet.get_btc_balance(online).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 99994601,
            future: 99994601,
            spendable: 99994601,
        }
    ));
    assert!(matches!(
        balances.colored,
        Balance {
            settled: 5000,
            future: 5000,
            spendable: 5000,
        }
    ));
}
