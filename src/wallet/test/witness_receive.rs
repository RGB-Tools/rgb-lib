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
        .witness_receive(
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
        party.get_wallet_data().bitcoin_network
    );
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    let (_, batch_transfer) = party.get_test_transfer_related(&transfer);
    assert_eq!(batch_transfer.min_confirmations, MIN_CONFIRMATIONS);

    // asset ID + expiration + 0 min confirmations
    let asset = party.issue_asset_cfa(None, None);
    let asset_id = asset.asset_id;
    let expiration_timestamp = (now().unix_timestamp() + expiration_secs) as u64;
    let min_confirmations = 0;
    let receive_data = party
        .wallet
        .witness_receive(
            Some(asset_id.clone()),
            Assignment::Fungible(amount),
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
    let invoice = Invoice::new(receive_data.invoice.clone()).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Cfa));

    // Invoice checks
    let invoice = Invoice::new(receive_data.invoice).unwrap();
    let invoice_data = invoice.invoice_data();
    assert_eq!(invoice_data.recipient_id, receive_data.recipient_id);
    assert_eq!(invoice_data.asset_schema, Some(AssetSchema::Cfa));
    assert_eq!(invoice_data.asset_id, Some(asset_id));
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

    // check recipient ID
    let result = RecipientInfo::new(receive_data.recipient_id);
    assert!(result.is_ok());

    // transport endpoints: multiple endpoints
    let transport_endpoints = vec![
        format!("rpc://{}", "127.0.0.1:3000/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3001/json-rpc"),
        format!("rpc://{}", "127.0.0.1:3002/json-rpc"),
    ];
    let result = party.wallet.witness_receive(
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
fn fail() {
    initialize();

    let mut wallet = get_test_wallet(true, None);

    // 0 expiration
    let result = wallet
        .witness_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() - 1) as u64),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap_err();
    assert_matches!(result, Error::InvalidExpiration);
}

// invoice paid on-chain without its consignment being delivered, then paid again with a TX
// spending the first one's change: check refresh reconciles the orphaned first TXO
#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn orphaned_payment_recovery() {
    initialize();

    let amount: u64 = 66;
    let amount_sat: u64 = 1000;

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);

    let receive_data = rcv_party.witness_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);

    // 1st payment: donation send whose consignment gets lost (posted under a bogus recipient ID)
    println!("setting MOCK_CONSIGNMENT_RECIPIENT_ID");
    MOCK_CONSIGNMENT_RECIPIENT_ID.replace(Some(s!("lost-consignment")));
    let txid_1 = party
        .wallet
        .send(
            party.online,
            recipient_map.clone(),
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    mine(false);
    party.wait_for_refresh(Some(&asset.asset_id));

    // receiver sees no consignment; sync quarantines the TXO paying the invoice script
    rcv_party.list_unspents_with_sync(false);
    rcv_party.refresh_result(None, &[]).unwrap();
    let db_data = rcv_party.db_data(false);
    let orphan_txo = db_data.txos.iter().find(|t| t.txid == txid_1).unwrap();
    assert!(orphan_txo.pending_witness);
    assert!(rcv_party.get_asset_balance_result(&asset.asset_id).is_err());

    // 2nd payment: same invoice, spending the 1st TX's change, consignment delivered normally
    let txid_2 = party
        .wallet
        .send(
            party.online,
            recipient_map,
            true,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .unwrap()
        .txid;
    rcv_party.wait_for_refresh(None);
    mine(false);
    rcv_party.wait_for_refresh(None);
    // full scan to find the 2nd TX, since the 1st consumed the pending witness script
    rcv_party.sync(SyncOptions {
        keychain: SyncKeychain::Colored,
        strategy: SyncStrategy::FullScan,
    });

    // both payments recovered: quarantine lifted, allocation saved, balance complete
    let db_data = rcv_party.db_data(false);
    let orphan_txo = db_data.txos.iter().find(|t| t.txid == txid_1).unwrap();
    assert!(!orphan_txo.pending_witness);
    assert_eq!(
        rcv_party.get_asset_balance(&asset.asset_id).settled,
        amount * 2
    );
    let unspents = rcv_party.list_unspents_with_sync(false);
    let orphan_unspent = unspents
        .iter()
        .find(|u| u.utxo.outpoint.txid == txid_1)
        .unwrap();
    assert_eq!(orphan_unspent.utxo.btc_amount, amount_sat);
    assert!(orphan_unspent.rgb_allocations.iter().any(|a| {
        a.asset_id == Some(asset.asset_id.clone())
            && a.assignment == Assignment::Fungible(amount)
            && a.settled
    }));
    let regular_unspent = unspents
        .iter()
        .find(|u| u.utxo.outpoint.txid == txid_2)
        .unwrap();
    assert!(regular_unspent.rgb_allocations.iter().any(|a| {
        a.asset_id == Some(asset.asset_id.clone())
            && a.assignment == Assignment::Fungible(amount)
            && a.settled
    }));

    // recovered sats and allocation are spendable: send everything back
    let receive_data = party.blind_receive();
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(amount * 2),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid_3 = rcv_party.send_retry(&recipient_map);
    assert!(!txid_3.is_empty());
}
