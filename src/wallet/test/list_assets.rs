use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // no assets
    let assets = wallet.list_assets(vec![]).unwrap();
    assert_eq!(assets.rgb20.unwrap().len(), 0);

    // one issued rgb20 asset
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
    let rgb21_assets = assets.rgb21.unwrap();
    assert_eq!(rgb20_assets.len(), 1);
    assert_eq!(rgb21_assets.len(), 0);
    let asset = rgb20_assets.first().unwrap();
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

    // two issued rgb20 assets
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
    let rgb21_assets = assets.rgb21.unwrap();
    assert_eq!(rgb20_assets.len(), 2);
    assert_eq!(rgb21_assets.len(), 0);
    let asset = rgb20_assets.last().unwrap();
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

    // three issued assets: 2x rgb20 + 1x rgb21
    let asset_3 = wallet
        .issue_asset_rgb21(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT * 3],
            None,
            None,
        )
        .unwrap();
    let assets = wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = assets.rgb20.unwrap();
    let rgb21_assets = assets.rgb21.unwrap();
    assert_eq!(rgb20_assets.len(), 2);
    assert_eq!(rgb21_assets.len(), 1);
    let asset = rgb21_assets.last().unwrap();
    assert_eq!(asset.asset_id, asset_3.asset_id);
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.description, Some(DESCRIPTION.to_string()));
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3
        }
    );
    assert!(asset.parent_id.is_none());
    let empty_data_paths = vec![];
    assert_eq!(asset.data_paths, empty_data_paths);

    // test filter by asset type
    let assets = wallet.list_assets(vec![AssetType::Rgb20]).unwrap();
    dbg!(&assets.rgb20);
    dbg!(&assets.rgb21);
    assert_eq!(assets.rgb20.unwrap().len(), 2);
    assert!(assets.rgb21.is_none());

    let assets = wallet.list_assets(vec![AssetType::Rgb21]).unwrap();
    dbg!(&assets.rgb20);
    dbg!(&assets.rgb21);
    assert!(assets.rgb20.is_none());
    assert_eq!(assets.rgb21.unwrap().len(), 1);
}
