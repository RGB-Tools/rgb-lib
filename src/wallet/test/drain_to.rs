use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    // receiver wallet
    let rcv_wallet = get_test_wallet(true, None);

    // drain funded wallet with no allocation UTXOs
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let address = test_get_address(&rcv_wallet); // also updates backup_info
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    test_drain_to_keep(&wallet, &online, &address);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded noutxo after draining");
    assert_eq!(unspents.len(), 0);

    // issue asset (to produce an RGB allocation)
    fund_wallet(test_get_address(&wallet));
    test_create_utxos_default(&mut wallet, &online);
    test_issue_asset_nia(&mut wallet, &online, None);

    // drain funded wallet with RGB allocations
    test_drain_to_keep(&wallet, &online, &test_get_address(&rcv_wallet));
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (false)");
    assert_eq!(unspents.len() as u8, UTXO_NUM);
    test_drain_to_destroy(&wallet, &online, &test_get_address(&rcv_wallet));
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (true)");
    assert_eq!(unspents.len(), 0);
}

#[test]
#[parallel]
fn fail() {
    initialize();

    // wallets
    let (wallet, online) = get_empty_wallet!();
    let (rcv_wallet, rcv_online) = get_empty_wallet!();

    // drain empty wallet
    let result = test_drain_to_result(&wallet, &online, &test_get_address(&rcv_wallet), true);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // bad online object
    fund_wallet(test_get_address(&wallet));
    let result = test_drain_to_result(&wallet, &rcv_online, &test_get_address(&rcv_wallet), false);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // bad address
    let result = test_drain_to_result(&wallet, &online, "invalid address", false);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // fee min/max
    fund_wallet(test_get_address(&wallet));
    let result =
        test_drain_to_begin_result(&wallet, &online, &test_get_address(&rcv_wallet), true, 0.9);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = test_drain_to_begin_result(
        &wallet,
        &online,
        &test_get_address(&rcv_wallet),
        true,
        1000.1,
    );
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));

    // no private keys
    let (wallet, online) = get_funded_noutxo_wallet!(false, false);
    let result = test_drain_to_result(&wallet, &online, &test_get_address(&rcv_wallet), false);
    assert!(matches!(result, Err(Error::WatchOnly)));
}
