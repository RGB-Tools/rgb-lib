use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // no assets
    let assets = wallet.list_assets(vec![]).unwrap();
    assert_eq!(assets.rgb20.unwrap().len(), 0);

    // one issued RGB20 asset
    let asset_1 = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let assets = wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = assets.rgb20.unwrap();
    let rgb25_assets = assets.rgb25.unwrap();
    assert_eq!(rgb20_assets.len(), 1);
    assert_eq!(rgb25_assets.len(), 0);
    let asset = rgb20_assets.first().unwrap();
    assert_eq!(asset.asset_id, asset_1.asset_id);
    assert_eq!(asset.ticker, TICKER.to_string());
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT,
            future: AMOUNT,
            spendable: AMOUNT,
        }
    );

    // two issued RGB20 assets
    let asset_2 = wallet
        .issue_asset_rgb20(
            online.clone(),
            s!("TICKER2"),
            s!("NAME2"),
            PRECISION * 2,
            vec![AMOUNT * 2],
        )
        .unwrap();
    let assets = wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = assets.rgb20.unwrap();
    let rgb25_assets = assets.rgb25.unwrap();
    assert_eq!(rgb20_assets.len(), 2);
    assert_eq!(rgb25_assets.len(), 0);
    let asset = rgb20_assets.last().unwrap();
    assert_eq!(asset.asset_id, asset_2.asset_id);
    assert_eq!(asset.ticker, "TICKER2".to_string());
    assert_eq!(asset.name, "NAME2".to_string());
    assert_eq!(asset.precision, PRECISION * 2);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 2,
            future: AMOUNT * 2,
            spendable: AMOUNT * 2,
        }
    );

    // three issued assets: 2x RGB20 + 1x RGB25
    let asset_3 = wallet
        .issue_asset_rgb25(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 3],
            None,
        )
        .unwrap();
    let assets = wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = assets.rgb20.unwrap();
    let rgb25_assets = assets.rgb25.unwrap();
    assert_eq!(rgb20_assets.len(), 2);
    assert_eq!(rgb25_assets.len(), 1);
    let asset = rgb25_assets.last().unwrap();
    assert_eq!(asset.asset_id, asset_3.asset_id);
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.description, Some(DESCRIPTION.to_string()));
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    let empty_data_paths = vec![];
    assert_eq!(asset.data_paths, empty_data_paths);

    // test filter by asset type
    let assets = wallet.list_assets(vec![AssetIface::RGB20]).unwrap();
    assert_eq!(assets.rgb20.unwrap().len(), 2);
    assert!(assets.rgb25.is_none());

    let assets = wallet.list_assets(vec![AssetIface::RGB25]).unwrap();
    assert!(assets.rgb20.is_none());
    assert_eq!(assets.rgb25.unwrap().len(), 1);
}
