use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

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
