use super::*;

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn success() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let mut party_send = get_funded_noutxo_party!();
    let mut recv_party = get_empty_party!();

    // create 1 UTXO and send the rest
    party_send.create_utxos(false, Some(1), None, FEE_RATE, None);
    party_send.send_btc(&recv_party.get_address(), 99_998_200);

    // issue
    let asset = party_send.issue_asset_nia(Some(&[AMOUNT]));

    // prepare PSBT
    let address = BdkAddress::from_str(&recv_party.get_address()).unwrap();
    let mut tx_builder = party_send.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .add_recipient(
            address.assume_checked().script_pubkey(),
            BdkAmount::from_sat(amt_sat),
        )
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let mut psbt = tx_builder.finish().unwrap();
    let mut psbt_copy = psbt.clone();
    assert!(
        !psbt
            .unsigned_tx
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
    );
    assert!(psbt.proprietary.is_empty());

    // color PSBT
    assert_eq!(psbt.unsigned_tx.input.len(), 1);
    let mut output_map = HashMap::new();
    let output = psbt
        .unsigned_tx
        .output
        .iter()
        .enumerate()
        .find(|(_, o)| o.value.to_sat() == amt_sat)
        .unwrap();
    let vout = output.0 as u32;
    output_map.insert(vout, AMOUNT); // sending AMOUNT since color_psbt doesn't support change
    let asset_coloring_info = AssetColoringInfo {
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
    let (fascia, beneficiaries) = party_send
        .wallet
        .color_psbt(&mut psbt, coloring_info.clone())
        .unwrap();

    // check PSBT
    assert!(
        psbt.unsigned_tx
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
    );
    assert!(!psbt.proprietary.is_empty());
    let vout = vout + 1;

    // check fascia
    assert_eq!(fascia.bundles().len(), 1);
    let (_cid, bundle) = fascia.bundles().iter().next().unwrap();
    let im_keys = bundle.input_map.keys();
    assert_eq!(im_keys.len(), 1);
    let mut transitions = bundle.known_transitions.iter().map(|kt| &kt.transition);
    assert_eq!(transitions.len(), 1);
    let transition = transitions.next().unwrap();
    let assignments = &transition.assignments;
    assert_eq!(assignments.len(), 1);
    let (_, fungible) = assignments.iter().next().unwrap();
    let fungible = fungible.as_fungible();
    assert_eq!(fungible.len(), 1);
    let fungible = fungible.first().unwrap();
    let seal = fungible.revealed_seal().unwrap();
    let state = fungible.as_revealed_state();
    assert_eq!(seal.txid, TxPtr::WitnessTx);
    assert_eq!(seal.vout.into_u32(), vout);
    assert_eq!(seal.blinding, blinding);
    assert_eq!(state.as_u64(), AMOUNT);

    // check beneficiaries
    assert_eq!(beneficiaries.len(), 1);
    let (_cid, seals) = beneficiaries.first_key_value().unwrap();
    let seal = match seals.first().unwrap() {
        BuilderSeal::Revealed(r) => r,
        BuilderSeal::Concealed(_) => panic!("revealed expected"),
    };
    assert_eq!(seal.txid, TxPtr::WitnessTx);
    assert_eq!(seal.vout.into_u32(), vout);
    assert_eq!(seal.blinding, blinding);

    // color PSBT and consume
    let transfers = party_send
        .wallet
        .color_psbt_and_consume(&mut psbt_copy, coloring_info)
        .unwrap();

    // check that the two color_psbt* methods produce matching PSBTs (no additional changes)
    assert_eq!(psbt, psbt_copy);

    // push consignment to proxy
    let txid = psbt_copy.unsigned_tx.compute_txid().to_string();
    let transfers_dir = party_send.wallet.get_transfers_dir().join(&txid);
    let consignment_path = transfers_dir.join(CONSIGNMENT_FILE);
    std::fs::create_dir_all(&transfers_dir).unwrap();
    assert_eq!(transfers.len(), 1);
    transfers
        .first()
        .unwrap()
        .save_file(&consignment_path)
        .unwrap();
    party_send
        .wallet
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
    recv_party
        .wallet
        .accept_transfer(txid.clone(), vout, consignment_endpoint, blinding)
        .unwrap();

    // consume fascia
    party_send.wallet.consume_fascia(fascia, None).unwrap();
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn list_unspents_vanilla_success() {
    initialize();

    // wallets
    let mut party = get_empty_party!();

    // no unspents
    let bak_info_before = party.db_backup_info_opt();
    assert!(bak_info_before.is_none());
    let unspent_list = party.list_unspents_vanilla(None);
    let bak_info_after = party.db_backup_info_opt();
    assert!(bak_info_after.is_none());
    assert_eq!(unspent_list.len(), 0);

    let _guard = stop_mining();

    send_to_address(party.get_address());

    // one unspent, no confirmations
    let unspent_list = party.list_unspents_vanilla(None);
    assert_eq!(unspent_list.len(), 0);
    let unspent_list = party.list_unspents_vanilla(Some(0));
    assert_eq!(unspent_list.len(), 1);

    drop(_guard);
    mine(false);

    // one unspent, 1 confirmation
    let unspent_list = party.list_unspents_vanilla(None);
    assert_eq!(unspent_list.len(), 1);
    let unspent_list = party.list_unspents_vanilla(Some(0));
    assert_eq!(unspent_list.len(), 1);

    party.create_utxos_default();

    // one unspent (change), colored unspents not listed
    mine(false);
    let unspent_list = party.list_unspents_vanilla(None);
    assert_eq!(unspent_list.len(), 1);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn list_unspents_vanilla_skip_sync() {
    initialize();

    let mut party = get_empty_party!();

    fund_wallet(party.get_address());

    // no unspents if skipping sync
    let unspents = party
        .wallet
        .list_unspents_vanilla(party.online, MIN_CONFIRMATIONS, true)
        .unwrap();
    assert_eq!(unspents.len(), 0);

    // 1 unspent after manually syncing
    party
        .wallet
        .sync(
            party.online,
            SyncOptions {
                keychain: SyncKeychain::Vanilla {
                    lookback: INDEXER_SYNC_LOOKBACK as u32,
                },
                strategy: SyncStrategy::FastSync,
            },
        )
        .unwrap();
    let unspents = party
        .wallet
        .list_unspents_vanilla(party.online, MIN_CONFIRMATIONS, true)
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
    let mut party = get_funded_party!();
    let mut rcv_party = get_empty_party!();

    // NIA
    let nia_asset = party.issue_asset_nia(None);
    party.check_save_new_asset(
        &mut rcv_party,
        &nia_asset.asset_id,
        Assignment::Fungible(asset_amount),
    );
    assert!(rcv_party.db_check_asset_exists(&nia_asset.asset_id).is_ok());
    let asset_model = rcv_party.db_asset(&nia_asset.asset_id);
    assert_eq!(asset_model.id, nia_asset.asset_id);
    assert_eq!(asset_model.initial_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Nia);

    // CFA
    let cfa_asset = party.issue_asset_cfa(None, None);
    party.check_save_new_asset(
        &mut rcv_party,
        &cfa_asset.asset_id,
        Assignment::Fungible(asset_amount),
    );
    assert!(rcv_party.db_check_asset_exists(&cfa_asset.asset_id).is_ok());
    let asset_model = rcv_party.db_asset(&cfa_asset.asset_id);
    assert_eq!(asset_model.id, cfa_asset.asset_id);
    assert_eq!(asset_model.initial_supply, AMOUNT.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert!(asset_model.ticker.is_none());
    assert_eq!(asset_model.schema, AssetSchema::Cfa);

    // UDA
    let image_str = ["tests", "qrcode.png"].join(MAIN_SEPARATOR_STR);
    let uda_asset =
        party.issue_asset_uda(Some(DETAILS), Some(FILE_STR), vec![&image_str, FILE_STR]);
    party.create_utxos(false, None, None, FEE_RATE, None);
    party.check_save_new_asset(&mut rcv_party, &uda_asset.asset_id, Assignment::NonFungible);
    assert!(rcv_party.db_check_asset_exists(&uda_asset.asset_id).is_ok());
    let asset_model = rcv_party.db_asset(&uda_asset.asset_id);
    assert_eq!(asset_model.id, uda_asset.asset_id);
    assert_eq!(asset_model.initial_supply, 1.to_string());
    assert_eq!(asset_model.name, NAME);
    assert_eq!(asset_model.precision, PRECISION);
    assert_eq!(asset_model.ticker.unwrap(), TICKER);
    assert_eq!(asset_model.schema, AssetSchema::Uda);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn color_psbt_uda() {
    initialize();

    let nonce = 42u64;

    // wallets
    let mut party_send = get_funded_noutxo_party!();

    // create 1 UTXO and send the rest
    party_send.create_utxos(false, Some(1), None, FEE_RATE, None);
    let mut recv_party = get_empty_party!();
    party_send.send_btc(&recv_party.get_address(), 99_998_200);

    // issue
    let asset = party_send.issue_asset_uda(None, None, vec![]);

    // create a custom BDK wallet with p2wpkh descriptor to avoid p2tr outputs,
    // so that the OP_RETURN is appended at the end
    let mnemonic = Mnemonic::parse_in(
        Language::English,
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    )
    .unwrap();
    let xprv = Xpriv::new_master(BdkNetwork::Regtest, &mnemonic.to_seed("")).unwrap();
    let custom_bdk_wallet =
        BdkWallet::create(format!("wpkh({xprv}/0/*)"), format!("wpkh({xprv}/1/*)"))
            .network(BdkNetwork::Regtest)
            .create_wallet_no_persist()
            .unwrap();
    let p2wpkh_addr = custom_bdk_wallet
        .peek_address(KeychainKind::External, 0)
        .address;

    // prepare PSBT: drain all wallet UTXOs to the p2wpkh address (no p2tr outputs, no change)
    let mut tx_builder = party_send.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .drain_wallet()
        .drain_to(p2wpkh_addr.script_pubkey())
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let mut psbt = tx_builder.finish().unwrap();
    assert!(
        !psbt
            .unsigned_tx
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
    );
    assert!(psbt.proprietary.is_empty());

    // color PSBT
    assert_eq!(psbt.unsigned_tx.input.len(), 1);
    let mut output_map = HashMap::new();
    output_map.insert(0u32, 1u64); // UDA: assign to vout 0, amount 1
    let asset_coloring_info = AssetColoringInfo {
        output_map,
        static_blinding: None,
    };
    let asset_info_map: HashMap<ContractId, AssetColoringInfo> = HashMap::from_iter([(
        ContractId::from_str(&asset.asset_id).unwrap(),
        asset_coloring_info,
    )]);
    let coloring_info = ColoringInfo {
        asset_info_map,
        static_blinding: None,
        nonce: Some(nonce),
    };
    let (fascia, beneficiaries) = party_send
        .wallet
        .color_psbt(&mut psbt, coloring_info)
        .unwrap();

    // check PSBT: OP_RETURN is appended at the end
    assert!(
        psbt.unsigned_tx
            .output
            .iter()
            .any(|o| o.script_pubkey.is_op_return())
    );
    assert!(!psbt.proprietary.is_empty());
    assert!(
        psbt.unsigned_tx
            .output
            .last()
            .unwrap()
            .script_pubkey
            .is_op_return()
    );

    // check fascia
    assert_eq!(fascia.bundles().len(), 1);
    let (_cid, bundle) = fascia.bundles().iter().next().unwrap();
    let im_keys = bundle.input_map.keys();
    assert_eq!(im_keys.len(), 1);
    let mut transitions = bundle.known_transitions.iter().map(|kt| &kt.transition);
    assert_eq!(transitions.len(), 1);
    let transition = transitions.next().unwrap();
    let assignments = &transition.assignments;
    assert_eq!(assignments.len(), 1);
    let (_, structured) = assignments.iter().next().unwrap();
    let structured = structured.as_structured();
    assert_eq!(structured.len(), 1);
    let seal = structured.first().unwrap().revealed_seal().unwrap();
    assert_eq!(seal.txid, TxPtr::WitnessTx);
    assert_eq!(seal.vout.into_u32(), 0);

    // check beneficiaries
    assert_eq!(beneficiaries.len(), 1);
    let (_cid, seals) = beneficiaries.first_key_value().unwrap();
    let seal = match seals.first().unwrap() {
        BuilderSeal::Revealed(r) => r,
        BuilderSeal::Concealed(_) => panic!("revealed expected"),
    };
    assert_eq!(seal.txid, TxPtr::WitnessTx);
    assert_eq!(seal.vout.into_u32(), 0);
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn color_psbt_fail() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let mut party_send = get_funded_noutxo_party!();
    let mut recv_party = get_empty_party!();

    // create 1 UTXO and send the rest
    party_send.create_utxos(false, Some(1), None, FEE_RATE, None);
    party_send.send_btc(&recv_party.get_address(), 99_998_200);

    // issue
    let asset = party_send.issue_asset_nia(Some(&[AMOUNT]));

    // prepare PSBT
    let address = BdkAddress::from_str(&recv_party.get_address()).unwrap();
    let mut tx_builder = party_send.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .add_recipient(
            address.assume_checked().script_pubkey(),
            BdkAmount::from_sat(amt_sat),
        )
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let mut psbt = tx_builder.finish().unwrap();

    // prepare coloring data
    assert_eq!(psbt.unsigned_tx.input.len(), 1);
    let mut output_map = HashMap::new();
    let output = psbt
        .unsigned_tx
        .output
        .iter()
        .enumerate()
        .find(|(_, o)| o.value.to_sat() == amt_sat)
        .unwrap();
    output_map.insert(output.0 as u32, AMOUNT);

    // wrong contract ID
    let fake_cid = "rgb:Ar4ouaLv-b7f7Dc_-z5EMvtu-FA5KNh1-nlae~jk-8xMBo7E";
    let asset_coloring_info = AssetColoringInfo {
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
    let result = party_send.wallet.color_psbt(&mut psbt, coloring_info);
    assert!(
        matches!(result, Err(Error::Internal { details: m }) if m.contains(&format!("contract {fake_cid} is unknown")))
    );

    // wrong output map vout
    let fake_o_map: HashMap<u32, u64> = HashMap::from_iter([(666, AMOUNT)]);
    let asset_coloring_info = AssetColoringInfo {
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
    let result = party_send.wallet.color_psbt(&mut psbt, coloring_info);
    let msg = "invalid vout in output_map, does not exist in the given PSBT";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));

    // wrong output map amount
    let fake_o_map = output_map.keys().map(|k| (*k, 999u64)).collect();
    let asset_coloring_info = AssetColoringInfo {
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
    let result = party_send
        .wallet
        .color_psbt(&mut psbt, coloring_info.clone());
    let msg = "total amount in output_map (999) greater than available (666)";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn color_psbt_overflow_fail() {
    initialize();

    let amt_sat = 500;
    let blinding = 777;

    // wallets
    let mut party_send = get_funded_noutxo_party!();
    let mut recv_party = get_empty_party!();

    // create 1 UTXO and send the rest
    party_send.create_utxos(false, Some(1), None, FEE_RATE, None);
    party_send.send_btc(&recv_party.get_address(), 99_998_200);

    // issue
    let asset = party_send.issue_asset_nia(Some(&[AMOUNT]));

    // total amount in output_map overflows u64: two valid vouts whose amounts sum to
    // more than u64::MAX (the checked sum must error before reaching the available check)
    let address = BdkAddress::from_str(&recv_party.get_address()).unwrap();
    let mut tx_builder = party_send.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .add_recipient(
            address.assume_checked().script_pubkey(),
            BdkAmount::from_sat(amt_sat),
        )
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let mut psbt = tx_builder.finish().unwrap();
    let output_map: HashMap<u32, u64> = HashMap::from_iter([(0, u64::MAX), (1, 1)]);
    let asset_coloring_info = AssetColoringInfo {
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
    let result = party_send.wallet.color_psbt(&mut psbt, coloring_info);
    let msg = "total amount in output_map exceeds u64::MAX";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));

    // vout in output_map overflows u32 when shifted by 1 for the OP_RETURN output
    let address = BdkAddress::from_str(&recv_party.get_address()).unwrap();
    let mut tx_builder = party_send.wallet.bdk_wallet_mut().build_tx();
    tx_builder
        .add_recipient(
            address.assume_checked().script_pubkey(),
            BdkAmount::from_sat(amt_sat),
        )
        .fee_rate(FeeRate::from_sat_per_vb_u32(FEE_RATE as u32));
    let mut psbt = tx_builder.finish().unwrap();
    let output_map: HashMap<u32, u64> = HashMap::from_iter([(u32::MAX, AMOUNT)]);
    let asset_coloring_info = AssetColoringInfo {
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
    let result = party_send.wallet.color_psbt(&mut psbt, coloring_info);
    let msg = "vout in output_map is too large";
    assert!(matches!(result, Err(Error::InvalidColoringInfo { details: m }) if m == msg));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn post_consignment_fail() {
    initialize();

    // wallets
    let party = get_empty_party!();

    // fake data
    let fake_txid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let transfers_dir = party.wallet.get_transfers_dir().join(fake_txid);
    let consignment_path = transfers_dir.join(CONSIGNMENT_FILE);
    std::fs::create_dir_all(&transfers_dir).unwrap();
    std::fs::File::create(&consignment_path).unwrap();

    // proxy error
    let invalid_proxy_url = "http://127.6.6.6:7777/json-rpc";
    let result = party.wallet.post_consignment(
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
    let result = party.wallet.post_consignment(
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
    let verbose_unsupported =
        "verbose transactions are unsupported by the provided electrum service";
    assert_matches!(result, Err(Error::InvalidIndexer { details: m }) if m.contains(verbose_unsupported));
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

    let mut party = get_empty_party!();

    // invalid txid
    let consignment_endpoint = RgbTransport::from_str(&PROXY_ENDPOINT).unwrap();
    let result = party
        .wallet
        .accept_transfer(s!("invalidTxid"), 0, consignment_endpoint, 0);
    assert_matches!(result, Err(Error::InvalidTxid));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn get_tx_height_fail() {
    initialize();

    let party = get_empty_party!();

    // invalid txid
    let result = party.wallet.get_tx_height(s!("invalidTxid"));
    assert_matches!(result, Err(Error::InvalidTxid));
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn update_witnesses_success() {
    initialize();

    let party = get_empty_party!();

    let result = party.wallet.update_witnesses(0, vec![]);
    assert!(result.is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn upsert_witness_success() {
    initialize();

    let party = get_empty_party!();

    let result = party
        .wallet
        .upsert_witness(RgbTxid::from_str(FAKE_TXID).unwrap(), WitnessOrd::Tentative);
    assert!(result.is_ok());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn create_consignments_success() {
    initialize();

    let mut party = get_funded_party!();
    let mut rcv_party = get_funded_party!();

    let asset = party.issue_asset_nia(None);
    let receive_data = rcv_party.blind_receive_asset_expiry(None, None);
    let recipient_map = HashMap::from([(
        asset.asset_id.clone(),
        vec![Recipient {
            assignment: Assignment::Fungible(10),
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: None,
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let psbt = party.send_begin_result(&recipient_map).unwrap().psbt;
    let result = party.wallet.create_consignments(psbt.clone());
    assert!(result.is_ok());
    let psbt = Psbt::from_str(&psbt).unwrap();
    let txid = psbt.extract_tx().unwrap().compute_txid().to_string();
    let consignment_path = party
        .wallet
        .get_asset_transfer_dir(party.wallet.get_transfers_dir().join(txid), &asset.asset_id)
        .join(CONSIGNMENT_FILE);
    assert!(consignment_path.is_file());
}

#[cfg(feature = "electrum")]
#[test]
#[parallel]
fn offline() {
    initialize();

    let mut wallet = get_test_wallet(true, None);
    let result = wallet.list_unspents_vanilla(Online { id: 0 }, MIN_CONFIRMATIONS, false);
    assert_matches!(result, Err(Error::Offline));
}
