use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue RGB20 asset
    let asset = test_issue_asset_nia(&mut wallet, &online, None);

    // single transfer
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let transfer_list = test_list_transfers(&wallet, Some(&asset.asset_id));
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);

    drain_wallet(&wallet, &online);
    fund_wallet(test_get_address(&wallet));
    test_create_utxos_default(&mut wallet, &online);

    // issue RGB25 asset
    let asset = test_issue_asset_cfa(&mut wallet, &online, None, None);

    // single transfer
    let transfer_list = test_list_transfers(&wallet, Some(&asset.asset_id));
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
    let result = test_list_transfers_result(&wallet, Some("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
