use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // single transfer
    let transfer_list = wallet.list_transfers(asset.asset_id).unwrap();
    assert_eq!(transfer_list.len(), 1);
}

#[test]
fn fail() {
    initialize();

    let (wallet, _online) = get_funded_wallet!();

    // asset not found
    let result = wallet.list_transfers(s!("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
