use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let (mut wallet_send, online_send) = get_funded_noutxo_wallet!();
    let (mut wallet_recv, _online_recv) = get_empty_wallet!();

    // create 1 UTXO and drain the rest
    let num_created = test_create_utxos(&wallet_send, &online_send, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    test_drain_to_keep(&wallet_send, &online_send, &test_get_address(&wallet_recv));

    // issue
    let asset = test_issue_asset_nia(&mut wallet_send, &online_send, Some(&[AMOUNT]));

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
        iface: AssetIface::RGB20,
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
    };
    let (fascia, beneficiaries) = wallet_send
        .color_psbt(&mut psbt, coloring_info.clone(), false)
        .unwrap();

    // check PSBT
    assert!(psbt
        .unsigned_tx
        .output
        .iter()
        .any(|o| o.script_pubkey.is_op_return()));
    assert!(!psbt.proprietary.is_empty());

    // check fascia
    let Fascia { anchor, bundles } = fascia.clone();
    assert!(anchor.is_bitcoin());
    assert_eq!(bundles.len(), 1);
    let (_cid, bundle) = bundles.iter().next().unwrap();
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
    let transfers_dir = wallet_send.transfers_dir().join(&txid);
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
        .accept_transfer(txid, vout, consignment_endpoint, blinding, true)
        .unwrap();

    // consume fascia
    wallet_send.consume_fascia(fascia).unwrap();
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

    mine(true);

    // one unspent, 1 confirmation
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, Some(0));
    assert_eq!(unspent_list.len(), 1);

    test_create_utxos_default(&wallet, &online);

    // one unspent (change), colored unspents not listed
    mine(false);
    let unspent_list = test_list_unspents_vanilla(&wallet, &online, None);
    assert_eq!(unspent_list.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn save_new_asset_success() {
    initialize();
    let asset_amount: u64 = 66;

    // wallets
    let (mut wallet, online) = get_funded_wallet!();
    let (mut rcv_wallet, _rcv_online) = get_empty_wallet!();

    // NIA
    let nia_asset = test_issue_asset_nia(&mut wallet, &online, None);
    test_save_new_asset(
        &mut wallet,
        &online,
        &mut rcv_wallet,
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
    let cfa_asset = test_issue_asset_cfa(&mut wallet, &online, None, None);
    test_save_new_asset(
        &mut wallet,
        &online,
        &mut rcv_wallet,
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
    let image_str = ["tests", "qrcode.png"].join(&MAIN_SEPARATOR.to_string());
    let uda_asset = test_issue_asset_uda(
        &mut wallet,
        &online,
        Some(DETAILS),
        Some(file_str),
        vec![&image_str, file_str],
    );
    test_create_utxos(&wallet, &online, false, None, None, FEE_RATE);
    test_save_new_asset(
        &mut wallet,
        &online,
        &mut rcv_wallet,
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
    let (mut wallet_send, online_send) = get_funded_noutxo_wallet!();
    let (wallet_recv, _online_recv) = get_empty_wallet!();

    // create 1 UTXO and drain the rest
    let num_created = test_create_utxos(&wallet_send, &online_send, false, Some(1), None, FEE_RATE);
    assert_eq!(num_created, 1);
    test_drain_to_keep(&wallet_send, &online_send, &test_get_address(&wallet_recv));

    // issue
    let asset = test_issue_asset_nia(&mut wallet_send, &online_send, Some(&[AMOUNT]));

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
    let fake_cid = "rgb:2rW1x8L-ZFNxV9MEo-fZpcxcpHo-yfNC1Fx5u-pJyuiY1Yh-1DLhceq";
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB20,
        input_outpoints: vec![input_outpoint.into()],
        output_map: output_map.clone(),
        static_blinding: Some(blinding),
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> =
        HashMap::from_iter([(ContractId::from_str(fake_cid).unwrap(), asset_coloring_info)]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: Some(blinding),
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, false);
    assert!(
        matches!(result, Err(Error::Internal { details: m }) if m.contains(&format!("contract {fake_cid} is unknown")))
    );

    // wrong asset iface
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB25,
        input_outpoints: vec![input_outpoint.into()],
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
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, false);
    assert!(
        matches!(result, Err(Error::Internal { details: m }) if m.contains("doesn't implement interface"))
    );

    // wrong input txid
    let mut fake_input_op = input_outpoint;
    fake_input_op.txid =
        Txid::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB20,
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
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, false);
    let msg = "PSBT contains no contract information";
    assert!(matches!(result, Err(Error::Internal { details: m }) if m == msg));

    // wrong input vout
    let mut fake_input_op = input_outpoint;
    fake_input_op.vout = 666;
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB20,
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
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, false);
    let msg = "PSBT contains no contract information";
    assert!(matches!(result, Err(Error::Internal { details: m }) if m == msg));

    // wrong output map vout
    let fake_o_map: HashMap<u32, u64> = HashMap::from_iter([(666, AMOUNT)]);
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB20,
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
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, false);
    let msg = "invalid vout in output_map, does not exist in the given PSBT";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));

    // wrong output map amount
    let fake_o_map = output_map.keys().map(|k| (*k, 999u64)).collect();
    let asset_coloring_info = AssetColoringInfo {
        iface: AssetIface::RGB20,
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
    };
    let result = wallet_send.color_psbt(&mut psbt, coloring_info.clone(), false);
    let msg = "total amount in output_map greater than available";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));
    // output map amount still wrong but skipping check
    let result = wallet_send.color_psbt(&mut psbt, coloring_info, true);
    assert!(result.is_ok());
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
    let transfers_dir = wallet.transfers_dir().join(fake_txid);
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
    assert!(
        matches!(result, Err(Error::Proxy { details: m }) if m.contains("error sending request for url"))
    );

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

    let (mut wallet, online) = get_funded_wallet!();

    let asset_nia = test_issue_asset_nia(&mut wallet, &online, None);
    let asset_nia_cid = ContractId::from_str(&asset_nia.asset_id).unwrap();
    let result = wallet.save_new_asset(&AssetSchema::Cfa, asset_nia_cid, None);
    assert!(matches!(result, Err(Error::AssetIfaceMismatch)));
}
