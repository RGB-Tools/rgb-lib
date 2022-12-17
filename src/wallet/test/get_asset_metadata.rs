use super::*;

#[test]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    let asset_rgb20 = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
        )
        .unwrap();
    let transfers = wallet.list_transfers(asset_rgb20.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let blind_data = rcv_wallet.blind(None, None, None).unwrap();
    let recipient_map = HashMap::from([(
        asset_rgb20.asset_id.clone(),
        vec![Recipient {
            amount: 10,
            blinded_utxo: blind_data.blinded_utxo,
        }],
    )]);
    wallet.send(online.clone(), recipient_map, false).unwrap();
    rcv_wallet
        .refresh(rcv_online.clone(), None, vec![])
        .unwrap();
    let rgb20_metadata = rcv_wallet
        .get_asset_metadata(rcv_online, asset_rgb20.asset_id.clone())
        .unwrap();

    assert_eq!(rgb20_metadata.asset_type, AssetType::Rgb20);
    assert_eq!(rgb20_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(rgb20_metadata.name, NAME.to_string());
    assert_eq!(rgb20_metadata.precision, PRECISION);
    assert_eq!(rgb20_metadata.ticker.unwrap(), TICKER.to_string());
    assert_eq!(rgb20_metadata.description, None);
    assert_eq!(rgb20_metadata.parent_id, None);
    assert!((timestamp - rgb20_metadata.timestamp) < 30);

    let file_str = "README.md";
    let description = Some(DESCRIPTION.to_string());
    let parent_id = Some(asset_rgb20.asset_id);
    let asset_rgb121 = wallet
        .issue_asset_rgb121(
            online.clone(),
            NAME.to_string(),
            description.clone(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
            parent_id.clone(),
            Some(file_str.to_string()),
        )
        .unwrap();
    let transfers = wallet
        .list_transfers(asset_rgb121.asset_id.clone())
        .unwrap();
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let rgb121_metadata = wallet
        .get_asset_metadata(online, asset_rgb121.asset_id)
        .unwrap();

    assert_eq!(rgb121_metadata.asset_type, AssetType::Rgb121);
    assert_eq!(rgb121_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(rgb121_metadata.name, NAME.to_string());
    assert_eq!(rgb121_metadata.precision, PRECISION);
    assert_eq!(rgb121_metadata.ticker, None);
    assert_eq!(rgb121_metadata.description, description);
    assert_eq!(rgb121_metadata.parent_id, parent_id);
    assert!((timestamp - rgb121_metadata.timestamp) < 30);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, online) = get_empty_wallet!();

    let result = wallet.get_asset_metadata(online, s!(""));
    assert!(matches!(result, Err(Error::AssetNotFound(_))));
}
