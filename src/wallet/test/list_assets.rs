use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // no assets
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let assets = test_list_assets(&wallet, &[]);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert_eq!(assets.nia.unwrap().len(), 0);

    // one issued NIA asset
    let asset_1 = test_issue_asset_nia(&mut wallet, &online, None);
    let assets = test_list_assets(&wallet, &[]);
    let nia_assets = assets.nia.unwrap();
    let cfa_assets = assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(cfa_assets.len(), 0);
    let asset = nia_assets.first().unwrap();
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

    // two issued NIA assets
    let asset_2 = wallet
        .issue_asset_nia(s!("TICKER2"), s!("NAME2"), PRECISION * 2, vec![AMOUNT * 2])
        .unwrap();
    let assets = test_list_assets(&wallet, &[]);
    let nia_assets = assets.nia.unwrap();
    let cfa_assets = assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 2);
    assert_eq!(cfa_assets.len(), 0);
    let asset = nia_assets.last().unwrap();
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

    // three issued assets: 2x NIA + 1x CFA
    let asset_3 = test_issue_asset_cfa(&mut wallet, &online, Some(&[AMOUNT * 3]), None);
    let assets = test_list_assets(&wallet, &[]);
    let nia_assets = assets.nia.unwrap();
    let cfa_assets = assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 2);
    assert_eq!(cfa_assets.len(), 1);
    let asset = cfa_assets.last().unwrap();
    assert_eq!(asset.asset_id, asset_3.asset_id);
    assert_eq!(asset.name, NAME.to_string());
    assert_eq!(asset.details, Some(DETAILS.to_string()));
    assert_eq!(asset.precision, PRECISION);
    assert_eq!(
        asset.balance,
        Balance {
            settled: AMOUNT * 3,
            future: AMOUNT * 3,
            spendable: AMOUNT * 3,
        }
    );
    assert_eq!(asset.media, None);

    // test filter by asset type
    let assets = test_list_assets(&wallet, &[AssetSchema::Nia]);
    assert_eq!(assets.nia.unwrap().len(), 2);
    assert!(assets.cfa.is_none());

    let assets = test_list_assets(&wallet, &[AssetSchema::Cfa]);
    assert!(assets.nia.is_none());
    assert_eq!(assets.cfa.unwrap().len(), 1);
}
