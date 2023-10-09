use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue RGB20 asset
    let asset = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // single transfer
    let transfer_list = wallet.list_transfers(Some(asset.asset_id)).unwrap();
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);

    drain_wallet(&wallet, online.clone());
    fund_wallet(wallet.get_address().unwrap());
    test_create_utxos_default(&mut wallet, online.clone());

    // issue RGB25 asset
    let asset = wallet
        .issue_asset_cfa(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            None,
        )
        .unwrap();

    // single transfer
    let transfer_list = wallet.list_transfers(Some(asset.asset_id)).unwrap();
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);
}

#[test]
#[parallel]
fn fail() {
    let wallet = get_test_wallet(false, None);

    // asset not found
    let result = wallet.list_transfers(Some(s!("rgb1inexistent")));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
