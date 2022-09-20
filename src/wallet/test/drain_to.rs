use super::*;

#[test]
fn success() {
    initialize();

    // receiver wallet
    let (rcv_wallet, _rcv_online) = get_empty_wallet!();

    // drain funded wallet with no allocation utxos
    let (wallet, online) = get_funded_noutxo_wallet!();
    wallet._sync_db_txos().unwrap();
    wallet
        .drain_to(online, rcv_wallet.get_address(), false)
        .unwrap();
    mine();
    wallet._sync_db_txos().unwrap();
    let unspents = list_test_unspents(&wallet, "funded noutxo after draining");
    assert_eq!(unspents.len(), 0);

    // issue asset (to produce an RGB allocation)
    let (mut wallet, online) = get_funded_wallet!();
    wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    wallet._sync_db_txos().unwrap();

    // drain funded wallet with RGB allocations
    wallet
        .drain_to(online.clone(), rcv_wallet.get_address(), false)
        .unwrap();
    mine();
    wallet._sync_db_txos().unwrap();
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (false)");
    assert_eq!(unspents.len() as u8, UTXO_NUM);
    wallet
        .drain_to(online, rcv_wallet.get_address(), true)
        .unwrap();
    mine();
    wallet._sync_db_txos().unwrap();
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (true)");
    assert_eq!(unspents.len(), 0);
}

#[test]
fn fail() {
    initialize();

    // receiver wallet
    let (rcv_wallet, rcv_online) = get_empty_wallet!();

    // drain empty wallet
    let (wallet, online) = get_empty_wallet!();
    let result = wallet.drain_to(online, rcv_wallet.get_address(), true);
    assert!(matches!(result, Err(Error::InsufficientFunds)));

    // bad online object
    let (wallet, _online) = get_funded_noutxo_wallet!();
    wallet._sync_db_txos().unwrap();
    let result = wallet.drain_to(rcv_online, rcv_wallet.get_address(), false);
    assert!(matches!(result, Err(Error::InvalidOnline())));

    // bad address
    let (wallet, online) = get_funded_noutxo_wallet!();
    wallet._sync_db_txos().unwrap();
    let result = wallet.drain_to(online, s!("invalid address"), false);
    assert!(matches!(result, Err(Error::InvalidAddress(_))));

    // no private keys
    let (wallet, online) = get_funded_noutxo_wallet!(false, false);
    wallet._sync_db_txos().unwrap();
    let result = wallet.drain_to(online, rcv_wallet.get_address(), false);
    assert!(matches!(result, Err(Error::WatchOnly())));
}
