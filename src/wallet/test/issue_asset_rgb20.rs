use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // add a pending operation to an UTXO so spendable balance will be != settled / future
    let _blind_data = wallet.blind(None, None);

    let asset = wallet
        .issue_asset_rgb20(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
        )
        .unwrap();
    show_unspent_colorings(&wallet, "after issuance");
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT,
        }
    );
}

#[test]
fn multi_success() {
    initialize();

    let amounts: Vec<u64> = vec![111, 222, 333, 444, 555];
    let sum: u64 = amounts.iter().sum();

    let (mut wallet, online) = get_funded_wallet!();

    let asset = wallet
        .issue_asset_rgb20(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            amounts.clone(),
        )
        .unwrap();

    // check balance is the sum of the amounts
    assert_eq!(asset.balance.settled, sum);

    // check each allocation ends up on a different UTXO
    let unspents: Vec<Unspent> = wallet
        .list_unspents(true)
        .unwrap()
        .into_iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .collect();
    let mut outpoints: Vec<Outpoint> = vec![];
    for unspent in &unspents {
        let outpoint = unspent.utxo.outpoint.clone();
        assert!(!outpoints.contains(&outpoint));
        outpoints.push(outpoint);
    }
    assert_eq!(outpoints.len(), amounts.len());

    // check all allocations are of the same asset
    assert!(unspents
        .iter()
        .filter(|u| !u.rgb_allocations.is_empty())
        .all(|u| {
            u.rgb_allocations.len() == 1
                && u.rgb_allocations.first().unwrap().asset_id == Some(asset.asset_id.clone())
        }));
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // bad online object
    let (_other_wallet, other_online) = get_funded_wallet!();
    let result = wallet.issue_asset_rgb20(
        other_online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidOnline())));

    // invalid ticker: too short
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        s!(""),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid ticker: too long
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        s!("ABCDEFGHI"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid ticker: lowercase
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        s!("TiCkEr"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid ticker: unicode characters
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        s!("ticker with ℧nicode characters"),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidTicker(_))));

    // invalid name: too short
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        TICKER.to_string(),
        s!(""),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid name: too long
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        TICKER.to_string(),
        ("a").repeat(257),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid name: unicode characters
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        TICKER.to_string(),
        s!("name with ℧nicode characters"),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InvalidName(_))));

    // invalid precision
    let result = wallet.issue_asset_rgb20(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        19,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));

    // invalid amount list
    let result = wallet.issue_asset_rgb20(online, TICKER.to_string(), NAME.to_string(), 19, vec![]);
    assert!(matches!(result, Err(Error::NoIssuanceAmounts)));

    // insufficient funds
    let (mut empty_wallet, empty_online) = get_empty_wallet!();
    let result = empty_wallet.issue_asset_rgb20(
        empty_online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![AMOUNT],
    );
    assert!(matches!(result, Err(Error::InsufficientBitcoins)));

    // insufficient allocations
    let (mut wallet, online) = get_funded_noutxo_wallet!();
    let result = wallet.issue_asset_rgb20(
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
    let result = wallet.issue_asset_rgb20(
        online,
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        vec![0],
    );
    assert!(matches!(result, Err(Error::FailedIssuance(_))));
}
