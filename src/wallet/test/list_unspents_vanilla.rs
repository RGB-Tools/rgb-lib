use super::*;
use serial_test::parallel;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    // wallets
    let (wallet, online) = get_empty_wallet!();

    // no unspents
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());
    assert_eq!(unspent_list.len(), 0);

    stop_mining();

    send_to_address(test_get_address(&wallet));

    // one unspent, no confirmations
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 0);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, Some(0));
    assert_eq!(unspent_list.len(), 1);

    mine(true);

    // one unspent, 1 confirmation
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, Some(0));
    assert_eq!(unspent_list.len(), 1);

    test_create_utxos_default(&wallet, &online);

    // one unspent (change), colored unspents not listed
    mine(false);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
}
