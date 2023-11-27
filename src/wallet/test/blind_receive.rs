use crate::database::entities::transfer_transport_endpoint;
use crate::wallet::MAX_TRANSPORT_ENDPOINTS;
use sea_orm::EntityTrait;
use serial_test::parallel;

use super::*;

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
    let receive_data = test_blind_receive(&mut wallet);
    let bak_info_after = wallet.database.get_backup_info().unwrap().unwrap();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(receive_data.expiration_timestamp.is_some());
    let timestamp = now_timestamp + DURATION_RCV_TRANSFER as i64;
    assert!(receive_data.expiration_timestamp.unwrap() - timestamp <= 1);
    // TODO: this needs the network to be rendered in the invoice's string
    //       see https://github.com/RGB-WG/rgb-wallet/issues/103
    //let decoded_invoice = Invoice::new(receive_data.invoice).unwrap();
    //assert!(decoded_invoice.invoice_data.network.is_some());
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (_, batch_transfer) = get_test_transfer_related(&wallet, &transfer);
    assert_eq!(batch_transfer.min_confirmations, MIN_CONFIRMATIONS);

    // positive expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data = wallet
        .blind_receive(
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
        .blind_receive(
            None,
            None,
            Some(0),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    assert!(receive_data.expiration_timestamp.is_none());

    // custom min confirmations
    let min_confirmations = 2;
    let receive_data = wallet
        .blind_receive(
            None,
            None,
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
    let result = wallet.blind_receive(
        Some(asset_id.clone()),
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());

    // all set
    let now_timestamp = now().unix_timestamp();
    let result = wallet.blind_receive(
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
    let result = wallet.blind_receive(
        None,
        None,
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

#[test]
#[parallel]
fn respect_max_allocations() {
    initialize();

    let (mut wallet, _online) = get_funded_wallet!();

    let available_allocations = UTXO_NUM as u32 * MAX_ALLOCATIONS_PER_UTXO;
    let mut created_allocations = 0;
    for _ in 0..UTXO_NUM {
        let mut txo_list: HashSet<DbTxo> = HashSet::new();
        for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
            let receive_data = test_blind_receive(&mut wallet);
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

    let result = wallet.blind_receive(
        None,
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[test]
#[parallel]
fn expire() {
    initialize();

    let expiration = 1;
    let (mut wallet, online) = get_funded_wallet!();

    // check expiration
    let now_timestamp = now().unix_timestamp();
    let receive_data_1 = wallet
        .blind_receive(
            None,
            None,
            Some(expiration),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let timestamp = now_timestamp + expiration as i64;
    assert!(receive_data_1.expiration_timestamp.unwrap() - timestamp <= 1);

    // wait for expiration to be in the past
    std::thread::sleep(std::time::Duration::from_millis(
        expiration as u64 * 1000 + 2000,
    ));

    // trigger the expiration of pending transfers
    let _asset = test_issue_asset_nia(&mut wallet, &online, None);

    // check transfer is now in status Failed
    let transfer = get_test_transfer_recipient(&wallet, &receive_data_1.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);
}

#[test]
#[parallel]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount = 66;

    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, rcv_online) = get_funded_wallet!();

    // issue
    let asset = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_id = asset.asset_id;
    // get issuance UTXO
    let unspents = test_list_unspents(&wallet, None, false);
    let unspent_issue = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_id.clone()))
        })
        .unwrap();
    // send
    let receive_data = test_blind_receive(&mut rcv_wallet);
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
    let txid = test_send(&mut wallet, &online, &recipient_map);
    assert!(!txid.is_empty());

    // check blind doesn't get allocated to UTXO being spent
    let receive_data = test_blind_receive(&mut wallet);
    show_unspent_colorings(&wallet, "after 1st blind");
    let unspents = test_list_unspents(&wallet, None, false);
    let unspent_blind_1 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_1.utxo.outpoint);
    // remove blind
    test_fail_transfers_blind(&mut wallet, &online, &receive_data.recipient_id);
    test_delete_transfers(&wallet, Some(&receive_data.recipient_id), None, false);

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_wallet.refresh(rcv_online, None, vec![]).unwrap();
    test_refresh_asset(&mut wallet, &online, &asset_id);
    // check blind doesn't get allocated to UTXO being spent
    let _receive_data = test_blind_receive(&mut wallet);
    show_unspent_colorings(&wallet, "after 2nd blind");
    let unspents = test_list_unspents(&wallet, None, false);
    let unspent_blind_2 = unspents
        .iter()
        .find(|u| u.rgb_allocations.iter().any(|a| a.asset_id.is_none()))
        .unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_2.utxo.outpoint);
}

#[test]
#[parallel]
fn fail() {
    initialize();

    fn blind_receive_0exp_withte(
        wallet: &mut Wallet,
        transport_endpoints: Vec<String>,
    ) -> Result<ReceiveData, Error> {
        wallet.blind_receive(None, None, Some(0), transport_endpoints, MIN_CONFIRMATIONS)
    }

    let mut wallet = get_test_wallet(true, Some(1)); // using 1 max allocation per utxo
    let online = test_go_online(&mut wallet, true, None);

    // insufficient funds
    let result = wallet.blind_receive(
        None,
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
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

    fund_wallet(test_get_address(&wallet));
    mine(false);
    test_create_utxos(&mut wallet, &online, true, Some(1), None, FEE_RATE);

    // bad asset id
    let result = wallet.blind_receive(
        Some(s!("rgb1inexistent")),
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // cannot blind if all UTXOS already have an allocation
    let _asset = test_issue_asset_nia(&mut wallet, &online, None);
    let result = wallet.blind_receive(
        None,
        None,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // transport endpoints: malformed string
    fund_wallet(test_get_address(&wallet));
    test_create_utxos_default(&mut wallet, &online);
    let transport_endpoints = vec!["malformed".to_string()];
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown://{PROXY_HOST}")];
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: transport type supported by RgbInvoice but unsupported by rgb-lib
    let transport_endpoints = vec![format!("ws://{PROXY_HOST}")];
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
    assert!(matches!(result, Err(Error::UnsupportedTransportType)));

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
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
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
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
    let receive_data = blind_receive_0exp_withte(&mut wallet, transport_endpoints).unwrap();
    let transfer = get_test_transfer_recipient(&wallet, &receive_data.recipient_id);
    let (transfer_data, _) = get_test_transfer_data(&wallet, &transfer);
    let tte_data = wallet
        .database
        .get_transfer_transport_endpoints_data(transfer.idx)
        .unwrap();
    for (tte, _) in tte_data {
        block_on(
            transfer_transport_endpoint::Entity::delete_by_id(tte.idx)
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
    let result = blind_receive_0exp_withte(&mut wallet, transport_endpoints);
    let msg = s!("no duplicate transport endpoints allowed");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));
}

#[test]
#[parallel]
fn wrong_asset_fail() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_1, online_1) = get_funded_wallet!();
    let (mut wallet_2, online_2) = get_funded_wallet!();

    // issue one asset per wallet
    let asset_a = test_issue_asset_nia(&mut wallet_1, &online_1, None);
    let asset_b = test_issue_asset_nia(&mut wallet_2, &online_2, None);

    let receive_data_a = wallet_1
        .blind_receive(
            Some(asset_a.asset_id),
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
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
    let txid = test_send(&mut wallet_2, &online_2, &recipient_map);
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
    let rcv_transfers_b_result = test_list_transfers_result(&wallet_1, Some(&asset_b.asset_id));
    assert!(matches!(
        rcv_transfers_b_result,
        Err(Error::AssetNotFound { asset_id: _ })
    ));
}

#[test]
#[parallel]
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

#[test]
#[parallel]
fn multiple_receive_same_utxo() {
    initialize();

    let amount: u64 = 66;

    let (mut wallet_recv, online_recv) = get_funded_noutxo_wallet!();
    let (mut wallet_send_1, online_send_1) = get_funded_wallet!();
    let (mut wallet_send_2, online_send_2) = get_funded_wallet!();

    // create 1 colorable UTXO on receiver wallet
    let created = test_create_utxos(
        &mut wallet_recv,
        &online_recv,
        false,
        Some(1),
        None,
        FEE_RATE,
    );
    assert_eq!(created, 1);
    let unspents_recv = test_list_unspents(&wallet_recv, None, false);
    assert_eq!(unspents_recv.iter().filter(|u| u.utxo.colorable).count(), 1);

    // blind twice, yielding 2 invoices paying to the same UTXO
    let receive_data_1 = test_blind_receive(&mut wallet_recv);
    let receive_data_2 = test_blind_receive(&mut wallet_recv);

    // check both transfers are to be received on the same UTXO
    let transfers_recv = test_list_transfers(&wallet_recv, None);
    assert!(transfers_recv
        .windows(2)
        .all(|w| w[0].receive_utxo == w[1].receive_utxo));

    // issue + send from wallet_send_1 to wallet_recv blind 1
    let asset_1 = test_issue_asset_nia(&mut wallet_send_1, &online_send_1, None);
    let recipient_map_1 = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_1.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = test_send(&mut wallet_send_1, &online_send_1, &recipient_map_1);
    assert!(!txid_1.is_empty());

    // issue + send from wallet_send_2 to wallet_recv blind 2
    let asset_2 = test_issue_asset_nia(&mut wallet_send_2, &online_send_2, None);
    let recipient_map_2 = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            amount,
            recipient_data: RecipientData::BlindedUTXO(
                SecretSeal::from_str(&receive_data_2.recipient_id).unwrap(),
            ),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = test_send(&mut wallet_send_2, &online_send_2, &recipient_map_2);
    assert!(!txid_2.is_empty());

    // refresh receiver + check both RGB allocations are on the same UTXO
    test_refresh_all(&mut wallet_recv, &online_recv);
    let unspents_recv = test_list_unspents(&wallet_recv, None, false);
    let unspents_recv_colorable: Vec<&Unspent> =
        unspents_recv.iter().filter(|u| u.utxo.colorable).collect();
    assert_eq!(unspents_recv_colorable.len(), 1);
    let allocations = &unspents_recv_colorable.first().unwrap().rgb_allocations;
    assert_eq!(allocations.len(), 2);
    assert_eq!(
        allocations.first().unwrap().asset_id,
        Some(asset_1.asset_id)
    );
    assert_eq!(allocations.last().unwrap().asset_id, Some(asset_2.asset_id));
}
