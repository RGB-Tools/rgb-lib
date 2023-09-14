use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    let asset_nia = wallet
        .issue_asset_nia(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
        )
        .unwrap();
    let transfers = wallet.list_transfers(asset_nia.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let receive_data = rcv_wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
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
    let nia_metadata = rcv_wallet.get_asset_metadata(asset_nia.asset_id).unwrap();

    assert_eq!(nia_metadata.asset_iface, AssetIface::RGB20);
    assert_eq!(nia_metadata.asset_schema, AssetSchema::NIA);
    assert_eq!(nia_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(nia_metadata.name, NAME.to_string());
    assert_eq!(nia_metadata.precision, PRECISION);
    assert_eq!(nia_metadata.ticker.unwrap(), TICKER.to_string());
    assert_eq!(nia_metadata.description, None);
    assert!((timestamp - nia_metadata.timestamp) < 30);

    let file_str = "README.md";
    let description = None;
    let asset_cfa = wallet
        .issue_asset_cfa(
            online.clone(),
            NAME.to_string(),
            description.clone(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
            Some(file_str.to_string()),
        )
        .unwrap();
    let transfers = wallet.list_transfers(asset_cfa.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let cfa_metadata = wallet.get_asset_metadata(asset_cfa.asset_id).unwrap();

    assert_eq!(cfa_metadata.asset_iface, AssetIface::RGB25);
    assert_eq!(cfa_metadata.asset_schema, AssetSchema::CFA);
    assert_eq!(cfa_metadata.issued_supply, AMOUNT * 2);
    assert_eq!(cfa_metadata.name, NAME.to_string());
    assert_eq!(cfa_metadata.precision, PRECISION);
    assert_eq!(cfa_metadata.ticker, None);
    assert_eq!(cfa_metadata.description, description);
    assert!((timestamp - cfa_metadata.timestamp) < 30);
}

#[test]
#[parallel]
fn fail() {
    initialize();

    let (mut wallet, _online) = get_empty_wallet!();

    let result = wallet.get_asset_metadata(s!(""));
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
