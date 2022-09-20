use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(asset.balance.settled, AMOUNT);
}

#[test]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();

    let (mut wallet, online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            amounts,
        )
        .unwrap();

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different utxo
    let unspents = wallet.list_unspents(false).unwrap();
    let mut outpoints: Vec<Outpoint> = vec![];
    for unspent in unspents {
        let outpoint = unspent.utxo.outpoint;
        assert!(!outpoints.contains(&outpoint));
        outpoints.push(outpoint);
    }
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
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidOnline())));

    // invalid ticker
    let result = wallet.issue_asset(
        online.clone(),
        s!("ticker with ℧nicode characters"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidTicker(_))));

    // invalid name
    let result = wallet.issue_asset(
        online.clone(),
        TICKER.to_string(),
        s!("name with ℧nicode characters"),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidName(_))));

    // invalid precision
    let result = wallet.issue_asset(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        19,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid amount list
    let result = wallet.issue_asset(online, TICKER.to_string(), NAME.to_string(), 19, vec![]);
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // insufficient funds
    let (mut empty_wallet, empty_online) = get_empty_wallet!();
    let result = empty_wallet.issue_asset(
        empty_online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InsufficientFunds)));

    // insufficient allocations
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let result = wallet.issue_asset(
        online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
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
        online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![0],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));
}
