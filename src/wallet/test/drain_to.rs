use super::*;

#[test]
fn success() {
    initialize();

    // receiver wallet
    let rcv_wallet = get_test_wallet(true, None);

    // drain funded wallet with no allocation UTXOs
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    wallet
        .drain_to(online.clone(), rcv_wallet.get_address(), false, FEE_RATE)
        .unwrap();
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded noutxo after draining");
    assert_eq!(unspents.len(), 0);

    // issue asset (to produce an RGB allocation)
    fund_wallet(wallet.get_address());
    test_create_utxos_default(&mut wallet, online.clone());
    wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // drain funded wallet with RGB allocations
    wallet
        .drain_to(online.clone(), rcv_wallet.get_address(), false, FEE_RATE)
        .unwrap();
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (false)");
    assert_eq!(unspents.len() as u8, UTXO_NUM);
    wallet
        .drain_to(online, rcv_wallet.get_address(), true, FEE_RATE)
        .unwrap();
    mine(false);
    let unspents = list_test_unspents(&wallet, "funded with allocations after draining (true)");
    assert_eq!(unspents.len(), 0);
}

#[test]
fn fail() {
    initialize();

    // wallets
    let (wallet, online) = get_empty_wallet!();
    let (rcv_wallet, rcv_online) = get_empty_wallet!();

    // drain empty wallet
    let result = wallet.drain_to(online.clone(), rcv_wallet.get_address(), true, FEE_RATE);
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // bad online object
    fund_wallet(wallet.get_address());
    let result = wallet.drain_to(rcv_online, rcv_wallet.get_address(), false, FEE_RATE);
    assert!(matches!(result, Err(Error::CannotChangeOnline)));

    // bad address
    let result = wallet.drain_to(online.clone(), s!("invalid address"), false, FEE_RATE);
    assert!(matches!(result, Err(Error::InvalidAddress { details: _ })));

    // fee min/max
    fund_wallet(wallet.get_address());
    let result = wallet.drain_to_begin(online.clone(), rcv_wallet.get_address(), true, 0.9);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_LOW));
    let result = wallet.drain_to_begin(online, rcv_wallet.get_address(), true, 1000.1);
    assert!(matches!(result, Err(Error::InvalidFeeRate { details: m }) if m == FEE_MSG_HIGH));

    // no private keys
    let (wallet, online) = get_funded_noutxo_wallet!(false, false);
    let result = wallet.drain_to(online, rcv_wallet.get_address(), false, FEE_RATE);
    assert!(matches!(result, Err(Error::WatchOnly)));
}
