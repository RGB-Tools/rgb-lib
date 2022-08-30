use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // no assets
    let asset_list = wallet.list_assets().unwrap();
    assert_eq!(asset_list.len(), 0);

    // one issued asset
    let asset_1 = wallet
        .issue_asset(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            AMOUNT,
        )
        .unwrap();
    let asset_list = wallet.list_assets().unwrap();
    assert_eq!(asset_list.len(), 1);
    let asset = asset_list.first().unwrap();
    assert_eq!(asset.asset_id, asset_1.asset_id);
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT
        }
    );

    // two issued assets
    let asset_2 = wallet
        .issue_asset(
            online.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION * 2,
            AMOUNT * 2,
        )
        .unwrap();
    let asset_list = wallet.list_assets().unwrap();
    assert_eq!(asset_list.len(), 2);
    let asset = asset_list.last().unwrap();
    assert_eq!(asset.asset_id, asset_2.asset_id);
    assert_eq!(asset.ticker, "TICKER2".to_string());
    assert_eq!(asset.name, "NAME2".to_string());
    assert_eq!(asset.precision, PRECISION * 2);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2
        }
    );
}
