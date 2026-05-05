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
    let (mut wallet, _online) = get_funded_wallet!();

    // sync input params
    // - check online is correct
    let wrong_online = Online { id: 1 };
    let result = test_sync_result(&mut wallet, wrong_online, sync_options);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));
}
