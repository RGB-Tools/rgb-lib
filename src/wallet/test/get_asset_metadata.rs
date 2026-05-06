use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset_nia = party.issue_asset_nia(Some(&[AMOUNT, AMOUNT]));
    let transfers = party.list_transfers(Some(&asset_nia.asset_id));
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_nia.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(10),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    party.send_retry(&recipient_map);
    rcv_party.wait_for_refresh(None);
    let bak_info_before = party.db_backup_info();
    let nia_metadata = rcv_party.get_asset_metadata(&asset_nia.asset_id);
    let bak_info_after = party.db_backup_info();
    assert_eq!(
        bak_info_after.last_operation_timestamp,
        bak_info_before.last_operation_timestamp
    );

    assert_eq!(nia_metadata.asset_schema, AssetSchema::Nia);
    assert_eq!(nia_metadata.initial_supply, AMOUNT * 2);
    assert_eq!(nia_metadata.name, NAME.to_string());
    assert_eq!(nia_metadata.precision, PRECISION);
    assert_eq!(nia_metadata.ticker.unwrap(), TICKER.to_string());
    assert_eq!(nia_metadata.details, None);
    assert!((timestamp - nia_metadata.timestamp) < 30);

    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let asset_uda =
        party.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str, FILE_STR]);
    let transfers = party.list_transfers(Some(&asset_uda.asset_id));
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let uda_metadata = party.get_asset_metadata(&asset_uda.asset_id);

    assert_eq!(uda_metadata.asset_schema, AssetSchema::Uda);
    assert_eq!(uda_metadata.initial_supply, 1);
    assert_eq!(uda_metadata.name, NAME.to_string());
    assert_eq!(uda_metadata.precision, PRECISION);
    assert_eq!(uda_metadata.ticker, Some(TICKER.to_string()));
    assert_eq!(uda_metadata.details, Some(DETAILS.to_string()));
    assert!((timestamp - uda_metadata.timestamp) < 30);
    let token = uda_metadata.token.unwrap();
    assert_eq!(token.index, 0);
    assert!(token.ticker.is_none());
    assert!(token.name.is_none());
    assert!(token.details.is_none());
    assert!(token.embedded_media.is_none());
    assert_eq!(token.media.as_ref().unwrap().mime, "text/plain");
    assert_eq!(token.attachments.get(&0).unwrap().mime, "image/png");
    assert_eq!(token.attachments.get(&1).unwrap().mime, "text/plain");
    assert!(token.reserves.is_none());

    let details = None;
    let asset_cfa = party
        .wallet
        .issue_asset_cfa(
            NAME.to_string(),
            details.clone(),
            PRECISION,
            vec![AMOUNT, AMOUNT],
            Some(FILE_STR.to_string()),
        )
        .unwrap();
    let transfers = party.list_transfers(Some(&asset_cfa.asset_id));
    assert_eq!(transfers.len(), 1);
    let issuance = transfers.first().unwrap();
    let timestamp = issuance.created_at;
    let cfa_metadata = party.get_asset_metadata(&asset_cfa.asset_id);

    assert_eq!(cfa_metadata.asset_schema, AssetSchema::Cfa);
    assert_eq!(cfa_metadata.initial_supply, AMOUNT * 2);
    assert_eq!(cfa_metadata.name, NAME.to_string());
    assert_eq!(cfa_metadata.precision, PRECISION);
    assert_eq!(cfa_metadata.ticker, None);
    assert_eq!(cfa_metadata.details, details);
    assert!((timestamp - cfa_metadata.timestamp) < 30);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    let party = offline_party!(get_test_wallet(true, None));

    let result = party.get_asset_metadata_result("");
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));
}
