use super::*;

#[test]
fn success() {
    initialize();

    let amount = 69;
    let expiration = 60;
    let (mut wallet, online) = get_funded_wallet!();

    // default expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data = wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data = wallet
        .witness_receive(
            None,
            None,
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
            None,
            Some(0),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_none());

    // asset id is set
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
    let asset_id = asset.asset_id;
    let result = wallet.witness_receive(
        Some(asset_id.clone()),
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());

    // all set
    let now_timestamp = now().unix_timestamp();
    let result = wallet.witness_receive(
        Some(asset_id.clone()),
        Some(amount),
        Some(expiration),
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());
    let receive_data = result.unwrap();

    // Invoice checks
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let mut invoice_data = invoice.invoice_data();
    let invoice_from_data = Invoice::from_invoice_data(invoice_data.clone()).unwrap();
    let approx_expiry = now_timestamp + expiration as i64;
    assert_eq!(invoice.invoice_string(), invoice_from_data.invoice_string());
    assert_eq!(invoice_data.recipient_id, receive_data.recipient_id);
    assert_eq!(invoice_data.asset_id, Some(asset_id));
    assert_eq!(invoice_data.amount, Some(amount));
    assert_eq!(invoice_data.network, Some(BitcoinNetwork::Regtest));
    assert!(invoice_data.expiration_timestamp.unwrap() - approx_expiry <= 1);
    let invalid_asset_id = s!("invalid");
    invoice_data.asset_id = Some(invalid_asset_id.clone());
    let result = Invoice::from_invoice_data(invoice_data);
    assert!(matches!(result, Err(Error::InvalidAssetID { asset_id: a }) if a == invalid_asset_id));

    // check WitnessData ScriptBuf
    let result = ScriptBuf::from_hex(&receive_data.recipient_id);
    assert!(result.is_ok());

    // transport endpoints: multiple endpoints
    let transport_endpoints = vec![
        format!("rpc://{}", "127.0.0.1:3000/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3001/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3002/json-rpc"),
    ];
    let result = wallet.witness_receive(
        None,
        None,
        Some(0),
        transport_endpoints.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());
    let transfer = get_test_transfer_recipient(&wallet, &result.unwrap().recipient_id);
    let tce_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    assert_eq!(tce_data.len(), transport_endpoints.len());
}

#[test]
fn fail() {
    initialize();

    let (mut wallet, _) = get_empty_wallet!();

    // invalid invoice (missing network)
    let receive_data = wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let mut invoice_data = invoice.invoice_data();
    invoice_data.network = None;
    let result = Invoice::from_invoice_data(invoice_data);
    assert!(matches!(
        result,
        Err(Error::InvalidInvoiceData { details: _ })
    ));
}
