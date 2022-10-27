use super::*;

#[test]
fn rgb20_success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset_rgb20(
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
    let transfer = transfer_list.first().unwrap();
    assert_eq!(transfer.amount, AMOUNT);
    assert_eq!(transfer.status, TransferStatus::Settled);
}

#[test]
fn rgb21_success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();

    // issue asset
    let asset = wallet
        .issue_asset_rgb21(
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

    let (wallet, _online) = get_funded_wallet!();

    // asset not found
    let result = wallet.list_transfers(s!("rgb1inexistent"));
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
