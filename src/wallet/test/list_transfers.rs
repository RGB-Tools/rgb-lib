use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue RGB20 asset
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // single transfer
    let transfer_list = wallet.list_transfers(asset.asset_id).unwrap();
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);

    drain_wallet(&wallet, online.clone());
    fund_wallet(wallet.get_address());
    mine(false);
    wallet
        .create_utxos(online.clone(), false, None, None)
        .unwrap();

    // issue RGB121 asset
    let asset = wallet
        .issue_asset_rgb121(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            None,
            None,
        )
        .unwrap();

    // single transfer
    let transfer_list = wallet.list_transfers(asset.asset_id).unwrap();
    assert_eq!(transfer_list.len(), 1);
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);
}

#[test]
fn fail() {
    initialize();

    let wallet = get_test_wallet(false);

    // asset not found
    let result = wallet.list_transfers(s!("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
