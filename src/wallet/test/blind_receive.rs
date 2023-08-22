use crate::database::entities::transfer_transport_endpoint;
use crate::wallet::MAX_TRANSPORT_ENDPOINTS;
use sea_orm::EntityTrait;

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
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data = wallet
        .blind_receive(None, None, Some(expiration), TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + expiration as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);

    // 0 expiration
    let receive_data = wallet
        .blind_receive(None, None, Some(0), TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_none());

    // asset id is set
    let asset = wallet
        .issue_asset_rgb25(
            online,
            NAME.to_string(),
            Some(DESCRIPTION.to_string()),
            PRECISION,
            vec![AMOUNT],
            None,
        )
        .unwrap();
    let asset_id = asset.asset_id;
    let result = wallet.blind_receive(
        Some(asset_id.clone()),
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
    );
    assert!(result.is_ok());

    // all set
    let now_timestamp = now().unix_timestamp();
    let result = wallet.blind_receive(
        Some(asset_id.clone()),
        Some(amount),
        Some(expiration),
        TRANSPORT_ENDPOINTS.clone(),
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
    assert!(invoice_data.expiration_timestamp.unwrap() - approx_expiry <= 1);
    let invalid_asset_id = s!("invalid");
    invoice_data.asset_id = Some(invalid_asset_id.clone());
    let result = Invoice::from_invoice_data(invoice_data);
    assert!(matches!(result, Err(Error::InvalidAssetID { asset_id: a }) if a == invalid_asset_id));

    // check BlindedUTXO
    let result = BlindedUTXO::new(receive_data.recipient_id);
    assert!(result.is_ok());

    // transport endpoints: multiple endpoints
    let transport_endpoints = vec![
        format!("rpc://{}", "127.0.0.1:3000/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3001/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3002/json-rpc"),
    ];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints.clone());
    assert!(result.is_ok());
    let transfer = get_test_transfer_recipient(&wallet, &result.unwrap().recipient_id);
    let tce_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    assert_eq!(tce_data.len(), transport_endpoints.len());
}

#[test]
fn respect_max_allocations() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let available_allocations = UTXO_NUM as u32 * MAX_ALLOCATIONS_PER_UTXO;
    let mut created_allocations = 0;
    for _ in 0..UTXO_NUM {
        let mut txo_list: HashSet<DbTxo> = HashSet::new();
        for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
            let receive_data = wallet
                .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
                .unwrap();
            created_allocations += 1;
            let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
            let coloring = get_test_coloring(&wallet, transfer.asset_transfer_idx);
            let txo = get_test_txo(&wallet, coloring.txo_idx);
            txo_list.insert(txo);
        }

        // check allocations have been equally distributed between UTXOs
        assert_eq!(txo_list.len(), UTXO_NUM as usize);
    }
    assert_eq!(available_allocations, created_allocations);

    let result = wallet.blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone());
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[test]
fn expire() {
    initialize();

    let expiration = 1;
    let (mut wallet, online) = get_funded_wallet!();

    // check expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data_1 = wallet
        .blind_receive(None, None, Some(expiration), TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let timestamp = now_timestamp + expiration as i64;
    assert!(receive_data_1.expiration_timestamp.unwrap() - timestamp <= 1);

    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));

    // trigger the expiration of pending transfers
    let _asset = wallet
        .issue_asset_rgb20(
            online,
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    // check transfer is now in status Failed
    let transfer = get_test_transfer_recipient(&wallet, &receive_data_1.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_id = asset.asset_id;
    // get issuance UTXO
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspent_issue = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_id.clone()))
        })
        .unwrap();
    // send
    let receive_data = rcv_wallet
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data.recipient_id).unwrap(),
            ),
            amount,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet, &online, recipient_map);
    assert!(!txid.is_empty());

    // check blind doesn't get allocated to UTXO being spent
    let receive_data = wallet
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    show_unspent_colorings(&wallet, "after 1st blind");
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspent_blind_1 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_1.utxo.outpoint);
    // remove blind
    wallet
        .fail_transfers(
            online.clone(),
            Some(receive_data.recipient_id.clone()),
            None,
            false,
        )
        .unwrap();
    wallet
        .delete_transfers(Some(receive_data.recipient_id), None, false)
        .unwrap();

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    wallet
        .refresh(online, Some(asset_id.clone()), vec![])
        .unwrap();
    // check blind doesn't get allocated to UTXO being spent
    let _result = wallet
        .blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone())
        .unwrap();
    show_unspent_colorings(&wallet, "after 2nd blind");
    let unspents = wallet.list_unspents(None, false).unwrap();
    let unspent_blind_2 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_2.utxo.outpoint);
}

#[test]
fn fail() {
    initialize();

    let mut wallet = get_test_wallet(true, Some(1)); // using 1 max allocation per utxo
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();

    // insufficient funds
    let result = wallet.blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone());
    assert!(matches!(
        result,
        Err(Error::InsufficientBitcoins {
            needed: _,
            available: _
        })
    ));

    // invalid BlindedUTXO
    let result = BlindedUTXO::new(s!("invalid"));
    assert!(matches!(
        result,
        Err(Error::InvalidBlindedUTXO { details: _ })
    ));

    // invalid invoice
    let result = Invoice::new(s!("invalid"));
    assert!(matches!(result, Err(Error::InvalidInvoice { details: _ })));

    fund_wallet(wallet.get_address());
    mine(false);
    test_create_utxos(&mut wallet, online.clone(), true, Some(1), None, FEE_RATE);

    // bad asset id
    let result = wallet.blind_receive(
        Some(s!("rgb1inexistent")),
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // cannot blind if all UTXOS already have an allocation
    let _asset = wallet
        .issue_asset_rgb20(
            online.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let result = wallet.blind_receive(None, None, None, TRANSPORT_ENDPOINTS.clone());
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // transport endpoints: malformed string
    fund_wallet(wallet.get_address());
    test_create_utxos_default(&mut wallet, online.clone());
    let transport_endpoints = vec!["malformed".to_string()];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown://{PROXY_HOST}")];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: transport type supported by RgbInvoice but unsupported by rgb-lib
    let transport_endpoints = vec![format!("ws://{PROXY_HOST}")];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    assert!(matches!(result, Err(Error::UnsupportedTransportType)));

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    let msg = s!("must provide at least a transport endpoint");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: too many endpoints
    let transport_endpoints = vec![
        format!("rpc://127.0.0.1:3000/json-rpc"),
        format!("rpc://127.0.0.1:3001/json-rpc"),
        format!("rpc://127.0.0.1:3002/json-rpc"),
        format!("rpc://127.0.0.1:3003/json-rpc"),
    ];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    let msg = format!(
        "library supports at max {} transport endpoints",
        MAX_TRANSPORT_ENDPOINTS
    );
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: no endpoints for transfer > Failed
    let transport_endpoints = vec![format!("rpc://{PROXY_HOST}")];
    let receive_data = wallet
        .blind_receive(None, None, Some(0), transport_endpoints)
        .unwrap();
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    let tce_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    for (tce, _) in tce_data {
        block_on(
            transfer_transport_endpoint::Entity::delete_by_id(tce.idx)
                .exec(wallet.database.get_connection()),
        )
        .unwrap();
    }
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);
    wallet.refresh(online, None, vec![]).unwrap();
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);

    // transport endpoints: same endpoint repeated
    let transport_endpoints = vec![
        format!("rpc://{PROXY_HOST}"),
        format!("rpc://{PROXY_HOST}"),
        format!("rpc://{PROXY_HOST}"),
    ];
    let result = wallet.blind_receive(None, None, Some(0), transport_endpoints);
    let msg = s!("no duplicate transport endpoints allowed");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));
}

#[test]
fn wrong_asset_fail() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue one asset per wallet
    let asset_a = wallet_1
        .issue_asset_rgb20(
            online_1.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();
    let asset_b = wallet_2
        .issue_asset_rgb20(
            online_2.clone(),
            TICKER.to_string(),
            NAME.to_string(),
            PRECISION,
            vec![AMOUNT],
        )
        .unwrap();

    let receive_data_a = wallet_1
        .blind_receive(
            Some(asset_a.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
        )
        .unwrap();

    let recipient_map = HashMap::from([(
        asset_b.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_a.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send_default(&mut wallet_2, &online_2, recipient_map);
    assert!(!txid.is_empty());

    // transfer is pending
    let rcv_transfer_a = get_test_transfer_recipient(&wallet_1, &receive_data_a.recipient_id);
    let (rcv_transfer_data_a, _) = get_test_transfer_data(&wallet_1, &rcv_transfer_a);
    assert_eq!(
        rcv_transfer_data_a.status,
        TransferStatus::WaitingCounterparty
    );

    // transfer doesn't progress to status WaitingConfirmations on the receiving side
    wallet_1.refresh(online_1, None, vec![]).unwrap();
    wallet_2.refresh(online_2, None, vec![]).unwrap();

    // transfer has been NACKed
    let (rcv_transfer_data_a, _) = get_test_transfer_data(&wallet_1, &rcv_transfer_a);
    assert_eq!(rcv_transfer_data_a.status, TransferStatus::Failed);
    let rcv_transfers_b = wallet_1.list_transfers(asset_b.asset_id);
    assert!(matches!(
        rcv_transfers_b,
        Err(Error::AssetNotFound { asset_id: _ })
    ));
}

#[test]
fn new_transport_endpoint() {
    // correct JsonRpc endpoint
    let result = TransportEndpoint::new(PROXY_ENDPOINT.clone());
    assert!(result.is_ok());

    // unsupported endpoint
    let result = TransportEndpoint::new(format!("ws://{PROXY_HOST}"));
    assert!(matches!(result, Err(Error::UnsupportedTransportType)));

    // no transport type
    let result = TransportEndpoint::new(PROXY_HOST.to_string());
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // unknown transport type
    let result = TransportEndpoint::new(format!("unknown:{PROXY_HOST}"));
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // leading ':'
    let result = TransportEndpoint::new(format!(":rpc://{PROXY_HOST}"));
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));
}
