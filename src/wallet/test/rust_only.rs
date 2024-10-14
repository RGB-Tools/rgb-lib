use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let (wallet_send, online_send) = get_funded_noutxo_wallet!();
    let (mut wallet_recv, _online_recv) = get_empty_wallet!();

    // create 1 UTXO and drain the rest
    let num_created = test_create_utxos(&wallet_send, &online_send, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    test_drain_to_keep(&wallet_send, &online_send, &test_get_address(&wallet_recv));

    // issue
    let asset = test_issue_asset_nia(&wallet_send, &online_send, Some(&[AMOUNT]));

    // prepare PSBT
    let address = BdkAddress::from_str(&test_get_address(&wallet_recv)).unwrap();
    let mut tx_builder = wallet_send.bdk_wallet.build_tx();
    tx_builder
        .add_recipient(address.payload.script_pubkey(), amt_sat)
        .fee_rate(FeeRate::from_sat_per_vb(FEE_RATE));
    let mut psbt = tx_builder.finish().unwrap().0;
    let mut psbt_copy = psbt.clone();
    assert!(!psbt
        .unsigned_tx
        .output
        .iter()
        .any(|o| o.script_pubkey.is_op_return()));
    assert!(psbt.proprietary.is_empty());

    // color PSBT
    assert_eq!(psbt.unsigned_tx.input.len(), 1);
    let input_outpoint = psbt.unsigned_tx.input.first().unwrap().previous_output;
    let mut output_map = HashMap::new();
    let output = psbt
        .unsigned_tx
        .output
        .iter()
        .enumerate()
        .find(|(_, o)| o.value == amt_sat)
        .unwrap();
    let vout = output.0 as u32;
    output_map.insert(vout, AMOUNT); // sending AMOUNT since color_psbt doesn't support change
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![input_outpoint.into()],
        output_map,
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let (fascia, beneficiaries) = wallet_send
        .color_psbt(&mut psbt, coloring_info.clone())
        .unwrap();

    // check PSBT
    assert!(psbt
        .unsigned_tx
        .output
        .iter()
        .any(|o| o.script_pubkey.is_op_return()));
    assert!(!psbt.proprietary.is_empty());

    // check fascia
    let Fascia { bundles, .. } = fascia.clone();
    assert_eq!(bundles.len(), 1);
    let (_cid, bundle_dichotomy) = bundles.iter().next().unwrap();
    let bundle = bundle_dichotomy.first.clone();
    let im_keys = bundle.input_map.keys();
    assert_eq!(im_keys.len(), 1);
    let mut transitions = bundle.known_transitions.values();
    assert_eq!(transitions.len(), 1);
    let transition = transitions.next().unwrap();
    let assignments = &transition.assignments;
    assert_eq!(assignments.len(), 1);
    let (_, fungible) = assignments.iter().next().unwrap();
    let fungible = fungible.as_fungible();
    assert_eq!(fungible.len(), 1);
    let fungible = fungible.first().unwrap();
    let seal = fungible.revealed_seal().unwrap();
    let state = fungible.as_revealed_state().unwrap();
    assert!(seal.is_bitcoin());
    let blindseal = match seal {
        XChain::Bitcoin(a) => a,
        _ => panic!("bitcoin expected"),
    };
    assert_eq!(blindseal.method, CloseMethod::OpretFirst);
    assert_eq!(blindseal.txid, TxPtr::WitnessTx);
    assert_eq!(blindseal.vout.into_u32(), vout);
    assert_eq!(blindseal.blinding, blinding);
    assert_eq!(state.value.as_u64(), AMOUNT);

    // check beneficiaries
    assert_eq!(beneficiaries.len(), 1);
    let (_cid, seals) = beneficiaries.first_key_value().unwrap();
    let revealed = match seals.first().unwrap() {
        BuilderSeal::Revealed(r) => r,
        BuilderSeal::Concealed(_) => panic!("revealed expected"),
    };
    assert!(revealed.is_bitcoin());
    let blindseal = match revealed {
        XChain::Bitcoin(a) => a,
        _ => panic!("bitcoin expected"),
    };
    assert_eq!(blindseal.method, CloseMethod::OpretFirst);
    assert_eq!(blindseal.txid, TxPtr::WitnessTx);
    assert_eq!(blindseal.vout.into_u32(), vout);
    assert_eq!(blindseal.blinding, blinding);

    // color PSBT and consume
    let transfers = wallet_send
        .color_psbt_and_consume(&mut psbt_copy, coloring_info)
        .unwrap();

    // check that the two color_psbt* methods produce matching PSBTs (no additional changes)
    assert_eq!(psbt, psbt_copy);

    // push consignment to proxy
    let txid = psbt_copy.unsigned_tx.txid().to_string();
    let transfers_dir = wallet_send.get_transfers_dir().join(&txid);
    let consignment_path = transfers_dir.join(CONSIGNMENT_FILE);
    std::fs::create_dir_all(&transfers_dir).unwrap();
    assert_eq!(transfers.len(), 1);
    transfers
        .first()
        .unwrap()
        .save_file(&consignment_path)
        .unwrap();
    wallet_send
        .post_consignment(
            PROXY_URL,
            txid.clone(),
            consignment_path,
            txid.clone(),
            Some(vout),
        )
        .unwrap();

    // accept transfer
    let consignment_endpoint = RgbTransport::from_str(&PROXY_ENDPOINT).unwrap();
    wallet_recv
        .accept_transfer(txid.clone(), vout, consignment_endpoint, blinding)
        .unwrap();

    // consume fascia
    wallet_send
        .consume_fascia(fascia, RgbTxid::from_str(&txid).unwrap())
        .unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn list_unspents_vanilla_success() {
    initialize();

    // wallets
    let (wallet, online) = get_empty_wallet!();

    // no unspents
    let bak_info_before = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_before.is_none());
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    let bak_info_after = wallet.database.get_backup_info().unwrap();
    assert!(bak_info_after.is_none());
    assert_eq!(unspent_list.len(), 0);

    stop_mining();

    send_to_address(test_get_address(&wallet));

    // one unspent, no confirmations
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 0);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, Some(0));
    assert_eq!(unspent_list.len(), 1);

    mine(false, true);

    // one unspent, 1 confirmation
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, Some(0));
    assert_eq!(unspent_list.len(), 1);

    test_create_utxos_default(&wallet, &online);

    // one unspent (change), colored unspents not listed
    mine(false, false);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn list_unspents_vanilla_skip_sync() {
    initialize();

    let (wallet, online) = get_empty_wallet!();

    fund_wallet(test_get_address(&wallet));

    // no unspents if skipping sync
    let unspents = wallet
        .list_unspents_vanilla(online.clone(), MIN_CONFIRMATIONS, true)
        .unwrap();
    assert_eq!(unspents.len(), 0);

    // 1 unspent after manually syncing
    wallet.sync(online.clone()).unwrap();
    let unspents = wallet
        .list_unspents_vanilla(online.clone(), MIN_CONFIRMATIONS, true)
        .unwrap();
    assert_eq!(unspents.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn save_new_asset_success() {
    initialize();
    let asset_amount: u64 = 66;

    // wallets
    let (wallet, online) = get_funded_wallet!();
    let (rcv_wallet, _rcv_online) = get_empty_wallet!();

    // NIA
    let nia_asset = test_issue_asset_nia(&wallet, &online, None);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &nia_asset.asset_id,
        asset_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(nia_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(nia_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, nia_asset.asset_id);
    assert_eq!(asset_model.issued_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Nia);

    // CFA
    let cfa_asset = test_issue_asset_cfa(&wallet, &online, None, None);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &cfa_asset.asset_id,
        asset_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(cfa_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(cfa_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, cfa_asset.asset_id);
    assert_eq!(asset_model.issued_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert!(asset_model.ticker.is_none());
    assert_eq!(asset_model.schema, AssetSchema::Cfa);

    // UDA
    let uda_amount: u64 = 1;
    let file_str = "README.md";
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let uda_asset = test_issue_asset_uda(
        &wallet,
        &online,
        Some(DETAILS),
        Some(file_str),
        vec![&image_str, file_str],
    );
    test_create_utxos(&wallet, &online, false, None, None, FEE_RATE);
    test_save_new_asset(
        &wallet,
        &online,
        &rcv_wallet,
        &uda_asset.asset_id,
        uda_amount,
    );
    assert!(&rcv_wallet
        .database
        .check_asset_exists(uda_asset.asset_id.clone())
        .is_ok());
    let asset_model = rcv_wallet
        .database
        .get_asset(uda_asset.asset_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(asset_model.id, uda_asset.asset_id);
    assert_eq!(asset_model.issued_supply, 1.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Uda);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn color_psbt_fail() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let (wallet_send, online_send) = get_funded_noutxo_wallet!();
    let (wallet_recv, _online_recv) = get_empty_wallet!();

    // create 1 UTXO and drain the rest
    let num_created = test_create_utxos(&wallet_send, &online_send, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    test_drain_to_keep(&wallet_send, &online_send, &test_get_address(&wallet_recv));

    // issue
    let asset = test_issue_asset_nia(&wallet_send, &online_send, Some(&[AMOUNT]));

    // prepare PSBT
    let address = BdkAddress::from_str(&test_get_address(&wallet_recv)).unwrap();
    let mut tx_builder = wallet_send.bdk_wallet.build_tx();
    tx_builder
        .add_recipient(address.payload.script_pubkey(), amt_sat)
        .fee_rate(FeeRate::from_sat_per_vb(FEE_RATE));
    let mut psbt = tx_builder.finish().unwrap().0;

    // prepare coloring data
    assert_eq!(psbt.unsigned_tx.input.len(), 1);
    let input_outpoint = psbt.unsigned_tx.input.first().unwrap().previous_output;
    let mut output_map = HashMap::new();
    let output = psbt
        .unsigned_tx
        .output
        .iter()
        .enumerate()
        .find(|(_, o)| o.value == amt_sat)
        .unwrap();
    output_map.insert(output.0 as u32, AMOUNT);

    // wrong contract ID
    let fake_cid = "rgb:Ar4ouaLv-b7f7Dc!-z5EMvtu-FA5KNh1-nlae$jk-8xMBo7E";
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![input_outpoint.into()],
        output_map: output_map.clone(),
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> =
        HashMap::from_iter([(ContractId::from_str(fake_cid).unwrap(), asset_coloring_info)]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info);
    assert!(
        matches!(result, Err(Error::Internal { details: m }) if m.contains(&format!("contract {fake_cid} is unknown")))
    );

    // wrong input txid
    let mut fake_input_op = input_outpoint;
    fake_input_op.txid =
        Txid::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![fake_input_op.into()],
        output_map: output_map.clone(),
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info);
    let msg = "PSBT contains no contract information";
    assert!(matches!(result, Err(Error::Internal { details: m }) if m == msg));

    // wrong input vout
    let mut fake_input_op = input_outpoint;
    fake_input_op.vout = 666;
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![fake_input_op.into()],
        output_map: output_map.clone(),
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info);
    let msg = "PSBT contains no contract information";
    assert!(matches!(result, Err(Error::Internal { details: m }) if m == msg));

    // wrong output map vout
    let fake_o_map: HashMap<u32, u64> = HashMap::from_iter([(666, AMOUNT)]);
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![input_outpoint.into()],
        output_map: fake_o_map,
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info);
    let msg = "invalid vout in output_map, does not exist in the given PSBT";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));

    // wrong output map amount
    let fake_o_map = output_map.keys().map(|k| (*k, 999u64)).collect();
    let asset_coloring_info = AssetColoringInfo {
        input_outpoints: vec![input_outpoint.into()],
        output_map: fake_o_map,
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
        nonce: None,
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info.clone());
    let msg = "total amount in output_map (999) greater than available (666)";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn post_consignment_fail() {
    initialize();

    // wallets
    let wallet = get_test_wallet(false, None);

    // fake data
    let fake_txid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let transfers_dir = wallet.get_transfers_dir().join(fake_txid);
    let consignment_path = transfers_dir.join(CONSIGNMENT_FILE);
    std::fs::create_dir_all(&transfers_dir).unwrap();
    std::fs::File::create(&consignment_path).unwrap();

    // proxy error
    let invalid_proxy_url = "http://127.6.6.6:7777/json-rpc";
    let result = wallet.post_consignment(
        invalid_proxy_url,
        fake_txid.to_string(),
        consignment_path.clone(),
        fake_txid.to_string(),
        Some(0),
    );
    assert_matches!(
        result,
        Err(Error::Proxy { details: m })
        if m.contains("error sending request for url")
            || m.contains("request or response body error for url"));

    // invalid transport endpoint
    let invalid_proxy_url = &format!("http://{PROXY_HOST_MOD_API}");
    let result = wallet.post_consignment(
        invalid_proxy_url,
        fake_txid.to_string(),
        consignment_path.clone(),
        fake_txid.to_string(),
        Some(0),
    );
    assert!(
        matches!(result, Err(Error::InvalidTransportEndpoint { details: m }) if m == "invalid result")
    );
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn save_new_asset_fail() {
    initialize();

    let (wallet, online) = get_funded_wallet!();

    let asset_nia = test_issue_asset_nia(&wallet, &online, None);
    let asset_nia_cid = ContractId::from_str(&asset_nia.asset_id).unwrap();
    let result = wallet.save_new_asset(&AssetSchema::Cfa, asset_nia_cid, None);
    assert!(matches!(result, Err(Error::AssetIfaceMismatch)));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn check_indexer_url_electrum_success() {
    initialize();

    let result = check_indexer_url(ELECTRUM_URL, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Electrum));

    let result = check_indexer_url(ELECTRUM_2_URL, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Electrum));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn check_indexer_url_electrum_fail() {
    initialize();

    let result = check_indexer_url(ELECTRUM_BLOCKSTREAM_URL, BitcoinNetwork::Regtest);
    let verbose_unsupported = s!("verbose transactions are currently unsupported");
    assert_matches!(result, Err(Error::InvalidElectrum { details: m }) if m == verbose_unsupported);
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn check_indexer_url_esplora_success() {
    initialize();

    let result = check_indexer_url(ESPLORA_URL, BitcoinNetwork::Regtest);
    assert_matches!(result, Ok(IndexerProtocol::Esplora));
}

#[cfg(feature = "esplora")]
#[test]
#[parallel]
fn check_indexer_url_esplora_fail() {
    initialize();

    let result = check_indexer_url(PROXY_URL, BitcoinNetwork::Regtest);
    let invalid_indexer = s!("not a valid electrum nor esplora server");
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m == invalid_indexer);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn check_proxy_url_success() {
    initialize();

    assert!(check_proxy_url(PROXY_URL).is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn check_proxy_url_fail() {
    initialize();

    let result = check_proxy_url(PROXY_URL_MOD_PROTO);
    assert_matches!(result, Err(Error::InvalidProxyProtocol { version: _ }));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn accept_transfer_fail() {
    initialize();

    let (mut wallet, _online) = get_empty_wallet!();

    // invalid txid
    let consignment_endpoint = RgbTransport::from_str(&PROXY_ENDPOINT).unwrap();
    let result = wallet.accept_transfer(s!("invalidTxid"), 0, consignment_endpoint, 0);
    assert_matches!(result, Err(Error::InvalidTxid));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn get_tx_height_fail() {
    initialize();

    let (wallet, _online) = get_empty_wallet!();

    // invalid txid
    let result = wallet.get_tx_height(s!("invalidTxid"));
    assert_matches!(result, Err(Error::InvalidTxid));
}
