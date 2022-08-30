use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(asset.balance.settled, AMOUNT);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // bad online object
    let (_other_wallet, other_online) = get_funded_wallet!();
    let result = wallet.issue_asset(
        other_online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::InvalidOnline())));

    // invalid ticker
    let result = wallet.issue_asset(
        online.clone(),
        s!("ticker with ℧nicode characters"),
        NAME.to_string(),
        PRECISION,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::InvalidTicker(_))));

    // invalid name
    let result = wallet.issue_asset(
        online.clone(),
        TICKER.to_string(),
        s!("name with ℧nicode characters"),
        PRECISION,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::InvalidName(_))));

    // invalid precision
    let result = wallet.issue_asset(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        19,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // insufficient funds
    let (mut empty_wallet, empty_online) = get_empty_wallet!();
    let result = empty_wallet.issue_asset(
        empty_online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::InsufficientFunds)));

    // insufficient allocations
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let result = wallet.issue_asset(
        online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        AMOUNT,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[test]
#[ignore = "currently succeeds"]
fn zero_amount_fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // invalid amount
    let result = wallet.issue_asset(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        0,
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));
}
