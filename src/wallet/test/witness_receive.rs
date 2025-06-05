use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount = 69;
    let expiration = 60;
    let (mut wallet, online) = get_funded_wallet!();

    // default expiration + min confirmations
    let bak_info_before = wallet.database.get_backup_info().unwrap().unwrap();
    let now_timestamp = now().unix_timestamp();
    let receive_data = test_witness_receive(&mut wallet);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);
    let decoded_invoice = Invoice::new(receive_data.invoice).unwrap();
    assert_eq!(
        decoded_invoice.invoice_data.network,
        wallet.bitcoin_network()
    );
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (_, batch_transfer) = get_test_transfer_related(&wallet, &transfer);
    assert_eq!(batch_transfer.min_confirmations, MIN_CONFIRMATIONS);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data = wallet
        .witness_receive(
            None,
            Assignment::Any,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + expiration as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // 0 expiration
    let receive_data = wallet
        .witness_receive(
            None,
            Assignment::Any,
            Some(0),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_none());

    // custom min confirmations
    let min_confirmations = 2;
    let receive_data = wallet
        .witness_receive(
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (_, batch_transfer) = get_test_transfer_related(&wallet, &transfer);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);

    // asset id is set
    let asset = test_issue_asset_cfa(&mut wallet, &online, None, None);
    let asset_id = asset.asset_id;
    let result = wallet.witness_receive(
        Some(asset_id.clone()),
        Assignment::Any,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());

    // all set
    let now_timestamp = now().unix_timestamp();
    let result = wallet.witness_receive(
        Some(asset_id.clone()),
        Assignment::Fungible(amount),
        Some(expiration),
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());
    let receive_data = result.unwrap();

    // Invoice checks
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    let approx_expiry = now_timestamp + expiration as i64;
    assert_eq!(invoice_data.recipient_id, receive_data.recipient_id);
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Cfa));
    assert_eq!(invoice_data.asset_id, Some(asset_id));
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));
    assert_eq!(invoice_data.network, BitcoinNetwork::Regtest);
    assert!(invoice_data.expiration_timestamp.unwrap() - approx_expiry <= 1);
    assert_eq!(
        invoice_data.transport_endpoints,
        TRANSPORT_ENDPOINTS.clone()
    );

    // check recipient ID
    let result = RecipientInfo::new(receive_data.recipient_id);
    assert!(result.is_ok());

    // transport endpoints: multiple endpoints
    let transport_endpoints = vec![
        format!("rpc://{}", "127.0.0.1:3000/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3001/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3002/json-rpc"),
    ];
    let result = wallet.witness_receive(
        None,
        Assignment::Any,
        Some(0),
        transport_endpoints.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());
    let transfer = get_test_transfer_recipient(&wallet, &result.unwrap().recipient_id);
    let tte_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    assert_eq!(tte_data.len(), transport_endpoints.len());
}
