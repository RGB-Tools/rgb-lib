use super::*;
use serial_test::parallel;

#[test]
#[parallel]
fn success() {
    initialize();

    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, rcv_online) = get_funded_wallet!();

    let asset_nia = test_issue_asset_nia(&wallet, &online, Some(&[AMOUNT, AMOUNT]));
    let transfers = test_list_transfers(&wallet, Some(&asset_nia.asset_id));
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let receive_data = test_blind_receive(&rcv_wallet);
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
    test_send(&wallet, &online, &recipient_map);
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let nia_metadata = test_get_asset_metadata(&rcv_wallet, &asset_nia.asset_id);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    assert_eq!(nia_metadata.asset_iface, AssetIface::RGB20);
    assert_eq!(nia_metadata.asset_schema, AssetSchema::Nia);
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
    let transfers = test_list_transfers(&wallet, Some(&asset_cfa.asset_id));
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let cfa_metadata = test_get_asset_metadata(&wallet, &asset_cfa.asset_id);

    assert_eq!(cfa_metadata.asset_iface, AssetIface::RGB25);
    assert_eq!(cfa_metadata.asset_schema, AssetSchema::Cfa);
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

    let (wallet, _online) = get_empty_wallet!();

    let result = test_get_asset_metadata_result(&wallet, "");
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
