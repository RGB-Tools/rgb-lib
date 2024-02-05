use super::*;
use rgbstd::persistence::Inventory;
use serial_test::parallel;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();
    let asset_amount: u64 = 66;

    // wallets
    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_empty_wallet!();

    // NIA
    let nia_asset = test_issue_asset_nia(&wallet, &online, None);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &nia_asset.asset_id,
        asset_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(nia_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(nia_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, nia_asset.asset_id);
    assert_eq!(asset_model.issued_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Nia);

    // CFA
    let cfa_asset = test_issue_asset_cfa(&wallet, &online, None, None);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &cfa_asset.asset_id,
        asset_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(cfa_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(cfa_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, cfa_asset.asset_id);
    assert_eq!(asset_model.issued_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert!(asset_model.ticker.is_none());
    assert_eq!(asset_model.schema, AssetSchema::Cfa);

    // UDA
    let uda_amount: u64 = 1;
    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(&MAIN_SEPARATOR.to_string());
    let uda_asset = test_issue_asset_uda(
        &wallet,
        &online,
        Some(DETAILS),
        Some(file_str),
        vec![&image_str, file_str],
    );
    test_create_utxos(&wallet, &online, false, None, None, FEE_RATE);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &uda_asset.asset_id,
        uda_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(uda_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(uda_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, uda_asset.asset_id);
    assert_eq!(asset_model.issued_supply, 1.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Uda);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let (wallet, online) = get_funded_wallet!();

    let asset_nia = test_issue_asset_nia(&wallet, &online, None);
    let asset_nia_cid = ContractId::from_str(&asset_nia.asset_id).unwrap();
    let mut runtime = wallet.rgb_runtime().unwrap();
    let contract = runtime.stock.export_contract(asset_nia_cid).unwrap();
    let result = wallet.save_new_asset(&mut runtime, &AssetSchema::Cfa, asset_nia_cid, contract);
    assert!(matches!(result, Err(Error::AssetIfaceMismatch)));
}
