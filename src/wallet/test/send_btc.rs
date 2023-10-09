use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let amount: u64 = 1000;

    // wallets
    let (mut wallet, online) = get_empty_wallet!();
    let (rcv_wallet, rcv_online) = get_empty_wallet!();

    // initial balance
    stop_mining();
    fund_wallet(wallet.get_address().unwrap());
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

    // balance after send
    let txid = wallet
        .send_btc(
            online.clone(),
            rcv_wallet.get_address().unwrap(),
            amount,
            FEE_RATE,
        )
        .unwrap();
    assert!(!txid.is_empty());
    let balances = wallet.get_btc_balance(online.clone()).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 0,
            future: 99993388,
            spendable: 99993388,
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
    let rcv_balances = rcv_wallet.get_btc_balance(rcv_online.clone()).unwrap();
    assert!(matches!(
        rcv_balances.vanilla,
        Balance {
            settled: 0,
            future: 1000,
            spendable: 1000,
        }
    ));
    assert!(matches!(
        rcv_balances.colored,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));

    // balance after mining
    mine(true);
    let balances = wallet.get_btc_balance(online).unwrap();
    assert!(matches!(
        balances.vanilla,
        Balance {
            settled: 99993388,
            future: 99993388,
            spendable: 99993388,
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
    let rcv_balances = rcv_wallet.get_btc_balance(rcv_online).unwrap();
    assert!(matches!(
        rcv_balances.vanilla,
        Balance {
            settled: 1000,
            future: 1000,
            spendable: 1000,
        }
    ));
    assert!(matches!(
        rcv_balances.colored,
        Balance {
            settled: 0,
            future: 0,
            spendable: 0,
        }
    ));
}

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
        electrum_url: wallet.online_data.as_ref().unwrap().electrum_url.clone(),
    };
    let result = wallet.send_btc(
        wrong_online,
        rcv_wallet.get_address().unwrap(),
        amount,
        FEE_RATE,
    );
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // invalid address
    let result = wallet.send_btc(online.clone(), s!("invalid"), amount, FEE_RATE);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));
    let result = wallet.send_btc(
        online.clone(),
        testnet_rcv_wallet.get_address().unwrap(),
        amount,
        FEE_RATE,
    );
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // invalid amount
    let result = wallet.send_btc(
        online.clone(),
        rcv_wallet.get_address().unwrap(),
        0,
        FEE_RATE,
    );
    assert!(matches!(result, Err(Error::OutputBelowDustLimit)));

    // invalid fee rate
    let result = wallet.send_btc(
        online.clone(),
        rcv_wallet.get_address().unwrap(),
        amount,
        0.9,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = wallet.send_btc(online, rcv_wallet.get_address().unwrap(), amount, 1000.1);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));
}
