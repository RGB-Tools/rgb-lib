use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amount = 69;
    let expiration_secs = 60i64;
    let mut party = get_funded_party!();

    // only mandatory fields
    let bak_info_before = party.db_backup_info();
    let receive_data = party
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert!(receive_data.expiration_timestamp.is_none());
    let decoded_invoice = Invoice::new(receive_data.invoice).unwrap();
    assert_eq!(
        decoded_invoice.invoice_data.network,
        BitcoinNetwork::Regtest
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (_, batch_transfer) = party.get_test_transfer_related(&transfer);
    assert_eq!(batch_transfer.min_confirmations, MIN_CONFIRMATIONS);

    // asset ID (NIA) + expiration + 0 min confirmations
    let asset_nia = party.issue_asset_nia(None);
    let asset_nia_id = asset_nia.asset_id;
    let expiration_timestamp = (now().unix_timestamp() + expiration_secs) as u64;
    let min_confirmations = 0;
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_nia_id.clone()),
            Assignment::Any,
            Some(expiration_timestamp),
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
        )
        .unwrap();
    assert_eq!(
        receive_data.expiration_timestamp,
        Some(expiration_timestamp)
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (_, batch_transfer) = party.get_test_transfer_related(&transfer);
    assert_eq!(batch_transfer.min_confirmations, min_confirmations);
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Nia));

    // asset id is set (UDA)
    let asset_uda = party.issue_asset_uda(None, None, vec![]);
    let asset_uda_id = asset_uda.asset_id;
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_uda_id.clone()),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Uda));

    // asset id is set (CFA)
    let asset_cfa = party.issue_asset_cfa(None, None);
    let asset_cfa_id = asset_cfa.asset_id;
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_cfa_id.clone()),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Cfa));

    // asset id is set (IFA) + amount
    party.create_utxos_default(); // more UTXOs to have free alocation slots
    let asset_ifa = party.issue_asset_ifa(None, None, None);
    let asset_ifa_id = asset_ifa.asset_id;
    let expiration_timestamp = (now().unix_timestamp() + expiration_secs) as u64;
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_ifa_id.clone()),
            Assignment::Fungible(amount),
            Some(expiration_timestamp),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    // Invoice checks
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.recipient_id, receive_data.recipient_id);
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Ifa));
    assert_eq!(invoice_data.asset_id, Some(asset_ifa_id.clone()));
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));
    assert_eq!(invoice_data.network, BitcoinNetwork::Regtest);
    assert_eq!(
        invoice_data.expiration_timestamp,
        Some(expiration_timestamp)
    );
    assert_eq!(
        invoice_data.transport_endpoints,
        TRANSPORT_ENDPOINTS.clone()
    );

    // detect assignment: amount, NIA (CFA/IFA)
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_nia_id.clone()),
            Assignment::Fungible(amount),
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_ASSET_OWNER.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );

    // detect assignment: amount, no schema
    let receive_data = party
        .wallet
        .blind_receive(
            None,
            Assignment::Fungible(amount),
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_ASSET_OWNER.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::Fungible(amount))
    );

    // detect assignment: any, NIA (CFA)
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_nia_id.clone()),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::Fungible(0));
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_ASSET_OWNER.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(transfer.requested_assignment, Some(Assignment::Fungible(0)));

    // detect assignment: non fungible, UDA
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_uda_id.clone()),
            Assignment::NonFungible,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_ASSET_OWNER.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(transfer.requested_assignment, Some(Assignment::NonFungible));

    // detect assignment: any, UDA
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_uda_id.clone()),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_ASSET_OWNER.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(transfer.requested_assignment, Some(Assignment::NonFungible));

    // detect assignment: inflation right, IFA
    let receive_data = party
        .wallet
        .blind_receive(
            Some(asset_ifa_id.clone()),
            Assignment::InflationRight(amount),
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::InflationRight(amount));
    assert_eq!(
        invoice_data.assignment_name,
        Some(RGB_STATE_INFLATION_ALLOWANCE.to_string())
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(
        transfer.requested_assignment,
        Some(Assignment::InflationRight(amount))
    );

    // detect assignment: any, no schema
    let receive_data = party
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.assignment, Assignment::Any);
    assert_eq!(invoice_data.assignment_name, None);
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert_eq!(transfer.requested_assignment, Some(Assignment::Any));

    // invalid assignment: non fungible, IFA schema
    let result = party.wallet.blind_receive(
        Some(asset_ifa_id.clone()),
        Assignment::NonFungible,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert_matches!(result, Err(Error::InvalidAssignment));

    // check recipient ID
    let result = RecipientInfo::new(receive_data.recipient_id);
    assert!(result.is_ok());

    // transport endpoints: multiple endpoints
    let transport_endpoints = vec![
        format!("rpc://{}", "127.0.0.1:3000/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3001/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3002/json-rpc"),
    ];
    let result = party.wallet.blind_receive(
        None,
        Assignment::Any,
        None,
        transport_endpoints.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(result.is_ok());
    let transfer = party.get_test_transfer_recipient(&result.unwrap().recipient_id);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), transport_endpoints.len());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn respect_max_allocations() {
    initialize();

    let mut party = get_funded_party!();

    let available_allocations = UTXO_NUM as u32 * MAX_ALLOCATIONS_PER_UTXO;
    let mut created_allocations = 0;
    for _ in 0..UTXO_NUM {
        let mut txo_list: HashSet<Outpoint> = HashSet::new();
        for _ in 0..MAX_ALLOCATIONS_PER_UTXO {
            let receive_data = party.blind_receive();
            created_allocations += 1;
            let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
            let txo = if let RecipientTypeFull::Blind { unblinded_utxo } =
                transfer.recipient_type.unwrap()
            {
                unblinded_utxo
            } else {
                panic!("should be a Blind variant");
            };
            txo_list.insert(txo);
        }

        // check allocations have been equally distributed between UTXOs
        assert_eq!(txo_list.len(), UTXO_NUM as usize);
    }
    assert_eq!(available_allocations, created_allocations);

    let result = party.blind_receive_result();
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn pending_outgoing_transfer_fail() {
    initialize();

    let amount = 66;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    // issue
    let asset = party.issue_asset_nia(None);
    let asset_id = asset.asset_id;
    // get issuance UTXO
    let unspents = party.list_unspents(false);
    let unspent_issue = unspents
        .iter()
        .find(|u| {
            u.rgb_allocations
                .iter()
                .any(|a| a.asset_id == Some(asset_id.clone()))
        })
        .unwrap();
    // send
    let receive_data = rcv_party.blind_receive();
    let recipient_map = HashMap::from([(
        asset_id.clone(),
        vec![Recipient {
            recipient_id: receive_data.recipient_id,
            witness_data: None,
            assignment: Assignment::Fungible(amount),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // check blind doesn't get allocated to UTXO being spent
    let receive_data = party.blind_receive();
    party.show_unspent_colorings("after 1st blind");
    let unspents = party.list_unspents(false);
    let unspent_blind_1 = unspents.iter().find(|u| u.pending_blinded > 0).unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_1.utxo.outpoint);
    // remove transfer
    party.fail_transfers_single(receive_data.batch_transfer_idx);
    party.delete_transfers(Some(receive_data.batch_transfer_idx), false);

    // take transfer from WaitingCounterparty to WaitingConfirmations
    rcv_party.wait_for_refresh(None);
    party.wait_for_refresh(Some(&asset_id));
    // check blind doesn't get allocated to UTXO being spent
    let _receive_data = party.blind_receive();
    party.show_unspent_colorings("after 2nd blind");
    let unspents = party.list_unspents(false);
    let unspent_blind_2 = unspents.iter().find(|u| u.pending_blinded > 0).unwrap();
    assert_ne!(unspent_issue.utxo.outpoint, unspent_blind_2.utxo.outpoint);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn fail() {
    initialize();

    fn blind_receive_withte(
        wallet: &mut Wallet,
        transport_endpoints: Vec<String>,
    ) -> Result<ReceiveData, Error> {
        wallet.blind_receive(
            None,
            Assignment::Any,
            None,
            transport_endpoints,
            MIN_CONFIRMATIONS,
        )
    }

    let mut party = offline_party!(get_test_wallet(true, Some(1))); // using 1 max allocation per utxo
    let online = party.go_online(true, None);
    let mut party = party!(party.wallet, online);

    // insufficient funds
    let result = party.blind_receive_result();
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // invalid recipient ID
    let result = RecipientInfo::new(s!("invalid"));
    assert!(matches!(result, Err(Error::InvalidRecipientID)));

    fund_wallet(party.get_address());
    mine(false);
    party.create_utxos(true, Some(1), None, FEE_RATE, None);

    // expiration in the past
    let result = party
        .wallet
        .blind_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() - 1) as u64),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap_err();
    assert!(matches!(result, Error::InvalidExpiration));

    // bad asset id
    let result = party.wallet.blind_receive(
        Some(s!("rgb1inexistent")),
        Assignment::Any,
        None,
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    );
    assert!(matches!(result, Err(Error::AssetNotFound { asset_id: _ })));

    // cannot blind if all UTXOS already have an allocation
    let _asset = party.issue_asset_nia(None);
    let result = party.blind_receive_result();
    assert!(matches!(result, Err(Error::InsufficientAllocationSlots)));

    // transport endpoints: malformed string
    fund_wallet(party.get_address());
    party.create_utxos_default();
    let transport_endpoints = vec!["malformed".to_string()];
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: unknown transport type
    let transport_endpoints = vec![format!("unknown://{PROXY_HOST}")];
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoint { details: _ })
    ));

    // transport endpoints: transport type supported by RgbInvoice but unsupported by rgb-lib
    let transport_endpoints = vec![format!("ws://{PROXY_HOST}")];
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
    assert!(matches!(result, Err(Error::UnsupportedTransportType)));

    // transport endpoints: not enough endpoints
    let transport_endpoints = vec![];
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
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
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
    let msg = format!("library supports at max {MAX_TRANSPORT_ENDPOINTS} transport endpoints");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // transport endpoints: no endpoints for transfer > Failed
    let transport_endpoints = vec![format!("rpc://{PROXY_HOST}")];
    let receive_data = blind_receive_withte(&mut party.wallet, transport_endpoints).unwrap();
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    for (tte, _) in tte_data {
        party.db_del_transfer_transport_endpoint(tte.idx);
    }
    assert_eq!(transfer_data.status, TransferStatus::WaitingCounterparty);
    party.wait_for_refresh(None);
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (transfer_data, _) = party.get_test_transfer_data(&transfer);
    assert_eq!(transfer_data.status, TransferStatus::Failed);

    // transport endpoints: same endpoint repeated
    let transport_endpoints = vec![
        format!("rpc://{PROXY_HOST}"),
        format!("rpc://{PROXY_HOST}"),
        format!("rpc://{PROXY_HOST}"),
    ];
    let result = blind_receive_withte(&mut party.wallet, transport_endpoints);
    let msg = s!("no duplicate transport endpoints allowed");
    assert!(matches!(
        result,
        Err(Error::InvalidTransportEndpoints { details: m }) if m == msg
    ));

    // invoice: unsupported layer 1
    println!("setting MOCK_CHAIN_NET");
    MOCK_CHAIN_NET.replace(Some(ChainNet::LiquidTestnet));
    let recipient_data = party.blind_receive();
    let result = Invoice::new(recipient_data.invoice);
    assert!(matches!(result, Err(Error::UnsupportedLayer1 { layer_1: l }) if l == "liquid" ));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn wrong_asset_fail() {
    initialize();

    let amount: u64 = 66;

    let mut party_1 = get_funded_party!();
    let mut party_2 = get_funded_party!();

    // issue one asset per wallet
    let asset_a = party_1.issue_asset_nia(None);
    let asset_b = party_2.issue_asset_nia(None);

    let receive_data_a = party_1
        .wallet
        .blind_receive(
            Some(asset_a.asset_id),
            Assignment::Any,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap();

    let recipient_map = HashMap::from([(
        asset_b.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_a.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = party_2.send_retry(&recipient_map);
    assert!(!txid.is_empty());

    // transfer is pending
    let rcv_transfer_a = party_1.get_test_transfer_recipient(&receive_data_a.recipient_id);
    let (rcv_transfer_data_a, _) = party_1.get_test_transfer_data(&rcv_transfer_a);
    assert_eq!(
        rcv_transfer_data_a.status,
        TransferStatus::WaitingCounterparty
    );

    // transfer doesn't progress to status WaitingConfirmations on the receiving side
    party_1.wait_for_refresh(None);
    party_2.wait_for_refresh(None);

    // transfer has been NACKed
    let (rcv_transfer_data_a, _) = party_1.get_test_transfer_data(&rcv_transfer_a);
    assert_eq!(rcv_transfer_data_a.status, TransferStatus::Failed);
    let rcv_transfers_b_result = party_1.list_transfers_result(Some(&asset_b.asset_id));
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

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn multiple_receive_same_utxo() {
    initialize();

    let amount: u64 = 66;

    let mut party_recv = get_funded_noutxo_party!();
    let mut party_send_1 = get_funded_party!();
    let mut party_send_2 = get_funded_party!();

    // create 1 colorable UTXO on receiver wallet
    party_recv.create_utxos(false, Some(1), None, FEE_RATE, None);
    let unspents_recv = party_recv.list_unspents(false);
    assert_eq!(unspents_recv.iter().filter(|u| u.utxo.colorable).count(), 1);

    // blind twice, yielding 2 invoices paying to the same UTXO
    let receive_data_1 = party_recv.blind_receive();
    let receive_data_2 = party_recv.blind_receive();

    // check both transfers are to be received on the same UTXO
    let transfers_recv = party_recv.list_transfers(None);
    assert!(
        transfers_recv
            .windows(2)
            .all(|w| w[0].receive_utxo == w[1].receive_utxo)
    );

    // issue + send from wallet_send_1 to wallet_recv blind 1
    let asset_1 = party_send_1.issue_asset_nia(None);
    let recipient_map_1 = HashMap::from([(
        asset_1.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_1.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_1 = party_send_1.send_retry(&recipient_map_1);
    assert!(!txid_1.is_empty());

    // issue + send from wallet_send_2 to wallet_recv blind 2
    let asset_2 = party_send_2.issue_asset_nia(None);
    let recipient_map_2 = HashMap::from([(
        asset_2.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data_2.recipient_id,
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_2 = party_send_2.send_retry(&recipient_map_2);
    assert!(!txid_2.is_empty());

    // refresh receiver + check both RGB allocations are on the same UTXO
    party_recv.wait_for_refresh(None);
    let unspents_recv = party_recv.list_unspents(false);
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

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn invoice_new() {
    // schema IDs (for invoices)
    let mut cfa_sid = SCHEMA_ID_CFA.to_string();
    cfa_sid.drain(0..8);
    let cfa_sid = &cfa_sid[..cfa_sid.find('#').unwrap()];
    let mut ifa_sid = SCHEMA_ID_IFA.to_string();
    ifa_sid.drain(0..8);
    let ifa_sid = &ifa_sid[..ifa_sid.find('#').unwrap()];
    let mut nia_sid = SCHEMA_ID_NIA.to_string();
    nia_sid.drain(0..8);
    let nia_sid = &nia_sid[..nia_sid.find('#').unwrap()];
    let mut uda_sid = SCHEMA_ID_UDA.to_string();
    uda_sid.drain(0..8);
    let uda_sid = &uda_sid[..uda_sid.find('#').unwrap()];

    // blinded UTXO
    let blinded = "bcrt:utxob:tjVmHbI2-U0_umHn-bU4cmP6-l3VW00H-ewoi2uz-XZG6O3i-wUFBW";

    // states
    let amount = 1u64;
    let amount_str = "ae";
    let data_str = "1@0";

    // invalid invoice (invalid string)
    let result = Invoice::new(s!("invalid"));
    assert!(matches!(result, Err(Error::InvalidInvoice { details: _ })));

    // invalid schema (CFA schema, Y characters changed Z)
    let cfa_sid_mod = cfa_sid.replace("Y", "Z");
    let invoice_str = format!("rgb:~/{cfa_sid_mod}/~/{blinded}");
    let result = Invoice::new(invoice_str.to_owned());
    assert!(
        matches!(result, Err(Error::InvalidInvoice { details: d }) if d == "invalid schema JgqK5hJX9ZBT4osCV7VcW_iLTcA5csUCnLzvaKTTrNZ.")
    );

    //
    // no schema
    //

    // amount, assetOwner
    let invoice_str = format!("rgb:~/~/{amount_str}/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));

    // amount, inflationAllowance
    let invoice_str = format!("rgb:~/~/{amount_str}/{blinded}?assignment_name=inflationAllowance");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::InflationRight(amount));

    // amount, unknown name
    let invoice_str = format!("rgb:~/~/{amount_str}/{blinded}?assignment_name=unknown");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Any);

    // data, assetOwner
    let invoice_str = format!("rgb:~/~/{data_str}/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // data, no name
    let invoice_str = format!("rgb:~/~/{data_str}/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // no state, no name
    let invoice_str = format!("rgb:~/~/~/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Any);

    // invalid invoice (unsupported assignment)
    let invoice_str = format!("rgb:~/~/{data_str}/{blinded}?assignment_name=unsupported");
    let result = Invoice::new(invoice_str.to_owned());
    assert!(
        matches!(result, Err(Error::InvalidInvoice { details: d }) if d == "unsupported assignment")
    );

    //
    // NIA or CFA
    //

    // amount, assetOwner
    let invoice_str = format!("rgb:~/{nia_sid}/{amount_str}/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));

    // amount, no name
    let invoice_str = format!("rgb:~/{cfa_sid}/{amount_str}/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));

    // no state, assetOwner
    let invoice_str = format!("rgb:~/{nia_sid}/~/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(0));

    // no state, no name
    let invoice_str = format!("rgb:~/{cfa_sid}/~/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(0));

    // invalid invoice (NIA/CFA invalid assignment)
    let invoice_str = format!("rgb:~/{nia_sid}/~/{blinded}?assignment_name=inflationAllowance");
    let result = Invoice::new(invoice_str.to_owned());
    assert!(
        matches!(result, Err(Error::InvalidInvoice { details: d }) if d == "invalid assignment")
    );

    //
    // UDA
    //

    // data, assetOwner
    let invoice_str = format!("rgb:~/{uda_sid}/{data_str}/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // data, no name
    let invoice_str = format!("rgb:~/{uda_sid}/{data_str}/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // no state, no name
    let invoice_str = format!("rgb:~/{uda_sid}/~/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // no state, no name
    let invoice_str = format!("rgb:~/{uda_sid}/~/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::NonFungible);

    // invalid invoice (UDA invalid assignment)
    let invoice_str = format!("rgb:~/{uda_sid}/~/{blinded}?assignment_name=inflationAllowance");
    let result = Invoice::new(invoice_str.to_owned());
    assert!(
        matches!(result, Err(Error::InvalidInvoice { details: d }) if d == "invalid assignment")
    );

    //
    // IFA
    //

    // amount, assetOwner
    let invoice_str = format!("rgb:~/{ifa_sid}/{amount_str}/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(amount));

    // no state, assetOwner
    let invoice_str = format!("rgb:~/{ifa_sid}/~/{blinded}?assignment_name=assetOwner");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Fungible(0));

    // amount, inflationAllowance
    let invoice_str =
        format!("rgb:~/{ifa_sid}/{amount_str}/{blinded}?assignment_name=inflationAllowance");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::InflationRight(amount));

    // no state, inflationAllowance
    let invoice_str = format!("rgb:~/{ifa_sid}/~/{blinded}?assignment_name=inflationAllowance");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::InflationRight(0));

    // amount, no name
    let invoice_str = format!("rgb:~/{ifa_sid}/~/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Any);

    // no state, no name
    let invoice_str = format!("rgb:~/{ifa_sid}/~/{blinded}");
    let invoice = Invoice::new(invoice_str.to_owned()).unwrap();
    let invoice_data = invoice.invoice_data;
    assert_eq!(invoice_data.assignment, Assignment::Any);

    // invalid invoice (IFA invalid assignment)
    let invoice_str = format!("rgb:~/{ifa_sid}/~/{blinded}?assignment_name=inexistent");
    let result = Invoice::new(invoice_str.to_owned());
    assert!(
        matches!(result, Err(Error::InvalidInvoice { details: d }) if d == "invalid assignment")
    );
}
