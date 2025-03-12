use super::*;

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn _success_common(wallet: &mut Wallet, online: &Online, esplora: bool) {
    fn random_send_btc(wallet: &mut Wallet, online: &Online) {
        let fee_rate = rand::rng().random_range(1..10);
        let amount = rand::rng().random_range(1000..5000);
        let mut attempts = 3;
        loop {
            let addr = test_get_address(wallet).to_string();
            if wallet
                .send_btc(online.clone(), addr, amount, fee_rate, true)
                .is_err()
            {
                attempts -= 1;
                if attempts == 0 {
                    println!("skipping send");
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            } else {
                break;
            }
        }
        wallet.sync(online.clone()).unwrap();
    }

    for _ in 0..100 {
        for _ in 0..15 {
            random_send_btc(wallet, online);
        }
        mine(esplora, false);
        for _ in 0..3 {
            random_send_btc(wallet, online);
        }
        if estimate_smart_fee(esplora) {
            break;
        }
    }

    let mut last_estimate = f64::MAX;
    for i in MIN_BLOCK_ESTIMATION..=MAX_BLOCK_ESTIMATION {
        let estimate = wallet.get_fee_estimation(online.clone(), i).unwrap();
        assert!(estimate <= last_estimate);
        last_estimate = estimate;
    }
}

#[cfg(feature = "electrum")]
#[test]
#[serial]
fn success_electrum() {
    initialize();

    let (mut wallet, online) = get_funded_noutxo_wallet!();

    _success_common(&mut wallet, &online, false);
}

#[cfg(feature = "esplora")]
#[test]
#[serial]
fn success_esplora() {
    initialize();

    let (mut wallet, online) = get_funded_noutxo_wallet!(ESPLORA_URL.to_string());

    _success_common(&mut wallet, &online, true);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn _fail_common(wallet: &Wallet, online: &Online, esplora: bool) {
    for _ in 0..100 {
        mine_blocks(esplora, 100, false);
        if let Err(e) = wallet.get_fee_estimation(online.clone(), 5) {
            assert!(matches!(e, Error::CannotEstimateFees));
            return;
        }
    }
    panic!("cannot find the expected error");
}

#[cfg(feature = "electrum")]
#[test]
#[ignore = "should be executed alone for performance reasons"]
#[serial]
fn fail_electrum() {
    initialize();

    let (wallet, online) = get_empty_wallet!();

    _fail_common(&wallet, &online, false)
}

#[cfg(feature = "esplora")]
#[test]
#[ignore = "should be executed alone for performance reasons"]
#[serial]
fn fail_esplora() {
    initialize();

    let (wallet, online) = get_empty_wallet!(ESPLORA_URL.to_string());

    _fail_common(&wallet, &online, true)
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (wallet, online) = get_empty_wallet!();

    // requested number of blocks too low
    let result = wallet.get_fee_estimation(online.clone(), MIN_BLOCK_ESTIMATION - 1);
    assert!(matches!(result, Err(Error::InvalidEstimationBlocks)));

    // requested number of blocks too high
    let result = wallet.get_fee_estimation(online, MAX_BLOCK_ESTIMATION + 1);
    assert!(matches!(result, Err(Error::InvalidEstimationBlocks)));
}
