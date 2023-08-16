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
    let receive_data = rcv_wallet
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_rgb20.asset_id.clone(),
        vec![Recipient {
            amount: 10,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    test_send_default(&mut wallet, &online, recipient_map);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    let rgb20_metadata = rcv_wallet.get_asset_metadata(asset_rgb20.asset_id).unwrap();

    assert_eq!(rgb20_metadata.asset_iface, AssetIface::RGB20);
    assert_eq!(rgb20_metadata.asset_schema, AssetSchema::NIA);
    assert_eq!(rgb20_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(rgb20_metadata.name, NAME.to_string());
    assert_eq!(rgb20_metadata.precision, PRECISION);
    assert_eq!(rgb20_metadata.ticker.unwrap(), TICKER.to_string());
    assert_eq!(rgb20_metadata.description, None);
    assert!((timestamp - rgb20_metadata.timestamp) < 30);

    let file_str = "README.md";
    let description = None;
    let asset_rgb25 = wallet
        .issue_asset_rgb25(
            online.clone(),
            NAME.to_string(),
            description.clone(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
            Some(file_str.to_string()),
        )
        .unwrap();
    let transfers = wallet.list_transfers(asset_rgb25.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let rgb25_metadata = wallet.get_asset_metadata(asset_rgb25.asset_id).unwrap();

    assert_eq!(rgb25_metadata.asset_iface, AssetIface::RGB25);
    assert_eq!(rgb25_metadata.asset_schema, AssetSchema::CFA);
    assert_eq!(rgb25_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(rgb25_metadata.name, NAME.to_string());
    assert_eq!(rgb25_metadata.precision, PRECISION);
    assert_eq!(rgb25_metadata.ticker, None);
    assert_eq!(rgb25_metadata.description, description);
    assert!((timestamp - rgb25_metadata.timestamp) < 30);
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, _online) = get_empty_wallet!();

    let result = wallet.get_asset_metadata(s!(""));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
