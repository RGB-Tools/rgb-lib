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
    let expiration_timestamp = default_rcv_expiration();
    let receive_data = party
        .wallet
        .witness_receive(
            None,
            Assignment::Any,
            expiration_timestamp,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            AddressReuse::New,
        )
        .unwrap();
    let bak_info_after = party.db_backup_info();
    assert!(bak_info_after.last_operation_timestamp > bak_info_before.last_operation_timestamp);
    assert_eq!(receive_data.expiration_timestamp, expiration_timestamp);
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
            expiration_timestamp,
            TRANSPORT_ENDPOINTS.clone(),
            min_confirmations,
            AddressReuse::New,
        )
        .unwrap();
    assert_eq!(receive_data.expiration_timestamp, expiration_timestamp);
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
        default_rcv_expiration(),
        transport_endpoints.clone(),
        MIN_CONFIRMATIONS,
        AddressReuse::New,
    );
    assert!(result.is_ok());
    let transfer = party.get_test_transfer_recipient(&result.unwrap().recipient_id);
    let tte_data = party.db_transfer_transport_endpoints_data(transfer.idx);
    assert_eq!(tte_data.len(), transport_endpoints.len());
}

#[test]
#[parallel]
fn fail() {
    let data_dir = PrivateDataDir::new();
    let mut wallet = data_dir.wallet(true, None);

    // 0 expiration
    let result = wallet
        .witness_receive(
            None,
            Assignment::Any,
            (now().unix_timestamp() - 1) as u64,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
            AddressReuse::New,
        )
        .unwrap_err();
    assert_matches!(result, Error::InvalidExpiration);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_address_busy() {
    initialize();

    let mut party = get_empty_party!();
    let receive_data = party.witness_receive();
    let script = script_buf_from_recipient_id(receive_data.recipient_id)
        .unwrap()
        .unwrap();
    let address = party.wallet.address_from_script(&script).to_string();

    // a fresh receive that is not final keeps its address
    let result = party.wallet.witness_receive(
        None,
        Assignment::Any,
        default_rcv_expiration(),
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
        AddressReuse::Existing(address.clone()),
    );
    assert_matches!(result, Err(Error::AddressBusy { address: a }) if a == address);
    let result = party
        .wallet
        .pin_address(Keychain::Colored, Some(address.clone()));
    assert_matches!(result, Err(Error::AddressBusy { address: a }) if a == address);

    // once it fails the address can be reused
    assert!(party.fail_transfers_single(receive_data.batch_transfer_idx));
    party.witness_receive_reuse(
        Assignment::Any,
        TRANSPORT_ENDPOINTS.clone(),
        default_rcv_expiration(),
        AddressReuse::Existing(address.clone()),
    );
    let pinned = party
        .wallet
        .pin_address(Keychain::Colored, Some(address.clone()))
        .unwrap();
    assert_eq!(pinned, address);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_fresh_invoice_unchanged() {
    // the values were produced by the library before address reuse
    let keys = crate::keys::restore_keys(
        BitcoinNetwork::Regtest,
        s!("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"),
        WitnessVersion::Taproot,
    )
    .unwrap();
    let data_dir = PrivateDataDir::new();
    let mut wallet = data_dir.wallet_raw(
        &SinglesigKeys::from_keys(&keys, None),
        None,
        BitcoinNetwork::Regtest,
    );
    let receive_data = wallet
        .witness_receive(
            None,
            Assignment::Fungible(100),
            4102444800,
            vec![s!("rpc://127.0.0.1:3000/json-rpc")],
            1,
            AddressReuse::New,
        )
        .unwrap();
    assert_eq!(
        receive_data.recipient_id,
        "bcrt:wvout:Be7zrr7L-VM9RP6X-pBRQduY-5ujGu~3-GqxY7TP-Tk5abpc-6cGRkrQ"
    );
    assert_eq!(
        receive_data.invoice,
        "rgb:~/~/BF/bcrt:wvout:Be7zrr7L-VM9RP6X-pBRQduY-5ujGu~3-GqxY7TP-Tk5abpc-6cGRkrQ?assignment_name=assetOwner&expiry=4102444800&endpoints=rpc://127.0.0.1:3000/json-rpc"
    );
    let party = offline_party!(wallet);
    let transfer = party.get_test_transfer_recipient(&receive_data.recipient_id);
    assert!(transfer.receive_dir.is_none());
    let script = script_buf_from_recipient_id(receive_data.recipient_id)
        .unwrap()
        .unwrap();
    assert!(
        party
            .db_pending_witness_scripts()
            .iter()
            .any(|p| p.script == script.to_hex_string())
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn reuse_invoice_shape() {
    initialize();

    let mut party = get_funded_party!();
    let receive_data = party.witness_receive_reuse(
        Assignment::Fungible(10),
        TRANSPORT_ENDPOINTS.clone(),
        default_rcv_expiration(),
        AddressReuse::Pinned,
    );
    let (plain_id, nonce) = receive_data.recipient_id.rsplit_once(':').unwrap();
    let nonce: u64 = nonce.parse().unwrap();

    // the nonce is a query parameter of today's invoice grammar
    let decoded = RgbInvoice::from_str(&receive_data.invoice).unwrap();
    assert_eq!(decoded.beneficiary.to_string(), plain_id);
    assert_eq!(decoded.unknown_query.get("nonce"), Some(&nonce.to_string()));

    // the recipient ID carries the nonce
    let invoice_data = Invoice::new(receive_data.invoice.clone())
        .unwrap()
        .invoice_data();
    assert_eq!(invoice_data.recipient_id, receive_data.recipient_id);
    assert!(invoice_data.unknown_query_params.is_empty());
    let recipient_info = RecipientInfo::new(receive_data.recipient_id.clone()).unwrap();
    assert_eq!(recipient_info.recipient_type, RecipientType::Witness);
    assert_eq!(
        script_buf_from_recipient_id(receive_data.recipient_id.clone()).unwrap(),
        script_buf_from_recipient_id(plain_id.to_string()).unwrap()
    );

    // an invalid nonce or a nonce on a blinded beneficiary is refused
    let invalid = receive_data
        .invoice
        .replace(&format!("nonce={nonce}"), "nonce=abc");
    let result = Invoice::new(invalid);
    assert_matches!(result, Err(Error::InvalidInvoice { .. }));
    let result = RecipientInfo::new(format!("{plain_id}:abc"));
    assert_matches!(result, Err(Error::InvalidRecipientID));
    let blind_data = party.blind_receive();
    let result = Invoice::new(format!("{}&nonce=5", blind_data.invoice));
    assert_matches!(result, Err(Error::InvalidInvoice { .. }));
    let result = RecipientInfo::new(format!("{}:5", blind_data.recipient_id));
    assert_matches!(result, Err(Error::InvalidRecipientID));
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
#[test]
#[parallel]
fn reuse_nonce_collision() {
    let data_dir = PrivateDataDir::new();
    let mut wallet = data_dir.wallet(false, None);
    let receive = |wallet: &mut Wallet| {
        wallet
            .witness_receive(
                None,
                Assignment::Any,
                4102444800,
                TRANSPORT_ENDPOINTS.clone(),
                MIN_CONFIRMATIONS,
                AddressReuse::Pinned,
            )
            .unwrap()
    };

    // a nonce already used by a receive is drawn again
    MOCK_NONCE.replace(vec![7, 7, 8]);
    let receive_data_1 = receive(&mut wallet);
    let (plain_id, _) = receive_data_1.recipient_id.rsplit_once(':').unwrap();
    let plain_id = plain_id.to_string();
    assert_eq!(receive_data_1.recipient_id, format!("{plain_id}:7"));
    let dir_1 = wallet
        .get_transfers_dir()
        .join(receive_data_1.recipient_id.replace(':', "_"));
    fs::create_dir_all(&dir_1).unwrap();
    fs::write(dir_1.join("file"), b"data").unwrap();
    let receive_data_2 = receive(&mut wallet);
    assert_eq!(receive_data_2.recipient_id, format!("{plain_id}:8"));

    // a nonce with a leftover directory is drawn again
    let leftover = wallet
        .get_transfers_dir()
        .join(format!("{plain_id}_9").replace(':', "_"));
    fs::create_dir_all(&leftover).unwrap();
    MOCK_NONCE.replace(vec![9, 10]);
    let receive_data_3 = receive(&mut wallet);
    assert_eq!(receive_data_3.recipient_id, format!("{plain_id}:10"));
    assert!(MOCK_NONCE.take().is_empty());

    // each receive has its own directory, earlier files stay
    let party = offline_party!(wallet);
    let receive_dirs: HashSet<String> = [&receive_data_1, &receive_data_2, &receive_data_3]
        .iter()
        .map(|rd| {
            party
                .get_test_transfer_recipient(&rd.recipient_id)
                .receive_dir
                .unwrap()
        })
        .collect();
    assert_eq!(receive_dirs.len(), 3);
    assert_eq!(fs::read(dir_1.join("file")).unwrap(), b"data");
}
