use super::*;

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn success_common(party: &mut SinglesigParty, esplora: bool) {
    fn random_send_btc(party: &mut SinglesigParty) {
        let fee_rate = rand::rng().random_range(1..10);
        let amount = rand::rng().random_range(1000..5000);
        let mut attempts = 3;
        loop {
            let addr = party.get_address().to_string();
            if party
                .wallet
                .send_btc(party.online, addr, amount, fee_rate, true)
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
    }

    for _ in 0..100 {
        for _ in 0..15 {
            random_send_btc(party);
        }
        mine(esplora);
        for _ in 0..3 {
            random_send_btc(party);
        }
        if estimate_smart_fee(esplora) {
            break;
        }
    }

    let mut last_estimate = f64::MAX;
    for i in MIN_BLOCK_ESTIMATION..=MAX_BLOCK_ESTIMATION {
        let estimate = party.wallet.get_fee_estimation(party.online, i).unwrap();
        assert!(estimate <= last_estimate);
        last_estimate = estimate;
    }
}

#[cfg(feature = "electrum")]
#[test]
#[ignore = "should be executed alone for performance reasons"]
#[serial]
fn success_electrum() {
    initialize();

    let mut party = get_funded_noutxo_party!();

    success_common(&mut party, false);
}

#[cfg(feature = "esplora")]
#[test]
#[ignore = "should be executed alone for performance reasons"]
#[serial]
fn success_esplora() {
    initialize();

    let mut party = get_funded_noutxo_party!(ESPLORA_URL.to_string());

    success_common(&mut party, true);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
fn fail_common(party: &SinglesigParty, esplora: bool) {
    for _ in 0..100 {
        mine_blocks(esplora, 100);
        if let Err(e) = party.wallet.get_fee_estimation(party.online, 5) {
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

    let party = get_empty_party!();

    fail_common(&party, false)
}

#[cfg(feature = "esplora")]
#[test]
#[ignore = "should be executed alone for performance reasons"]
#[serial]
fn fail_esplora() {
    initialize();

    let party = get_empty_party!(ESPLORA_URL.to_string());

    fail_common(&party, true)
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    // === offline tests

    let offline_party = {
        let wallet = get_test_wallet(true, None);
        party!(wallet, Online { id: 0 })
    };
    let result = offline_party
        .wallet
        .get_fee_estimation(Online { id: 0 }, MIN_BLOCK_ESTIMATION);
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let party = get_empty_party!();

    // requested number of blocks too low
    let result = party
        .wallet
        .get_fee_estimation(party.online, MIN_BLOCK_ESTIMATION - 1);
    assert!(matches!(result, Err(Error::InvalidEstimationBlocks)));

    // requested number of blocks too high
    let result = party
        .wallet
        .get_fee_estimation(party.online, MAX_BLOCK_ESTIMATION + 1);
    assert!(matches!(result, Err(Error::InvalidEstimationBlocks)));
}
