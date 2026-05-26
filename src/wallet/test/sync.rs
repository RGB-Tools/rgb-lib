use super::*;

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
    let result = offline_party.wallet.sync(
        Online { id: 0 },
        SyncOptions {
            keychain: SyncKeychain::Colored,
            strategy: SyncStrategy::FastSync,
        },
    );
    assert_matches!(result, Err(Error::Offline));

    // === online tests

    let sync_options = SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FastSync,
    };

    // wallets
    let mut party = get_funded_party!();

    // sync input params
    // - check online is correct
    let wrong_online = Online { id: 1 };
    let good_online = party.online;
    party.online = wrong_online;
    let result = party.sync_result(sync_options);
    party.online = good_online;
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}
