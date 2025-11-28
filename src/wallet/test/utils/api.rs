use super::*;

pub(crate) fn test_blind_receive(wallet: &mut Wallet) -> ReceiveData {
    test_blind_receive_result(wallet).unwrap()
}

pub(crate) fn test_blind_receive_result(wallet: &mut Wallet) -> Result<ReceiveData, Error> {
    wallet.blind_receive(
        None,
        Assignment::Any,
        Some((now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64),
        TRANSPORT_ENDPOINTS.clone(),
        MIN_CONFIRMATIONS,
    )
}

pub(crate) fn test_witness_receive(wallet: &mut Wallet) -> ReceiveData {
    wallet
        .witness_receive(
            None,
            Assignment::Any,
            Some((now().unix_timestamp() + DURATION_RCV_TRANSFER as i64) as u64),
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_create_utxos_default(wallet: &mut Wallet, online: Online) {
    test_create_utxos(wallet, online, false, None, None, FEE_RATE, None);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_create_utxos(
    wallet: &mut Wallet,
    online: Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: u64,
    expected: Option<u8>,
) {
    let unspents = test_list_unspents(wallet, Some(online), false);
    let colorable_before = unspents.iter().filter(|u| u.utxo.colorable).count();
    let expected = expected.unwrap_or(num.unwrap_or(UTXO_NUM));
    let _ = test_create_utxos_nowait(wallet, online, up_to, num, size, fee_rate);
    let check = || {
        let unspents = test_list_unspents(wallet, Some(online), false);
        let colorable = unspents.iter().filter(|u| u.utxo.colorable).count();
        if (colorable - colorable_before) == expected as usize {
            return true;
        }
        false
    };
    if !wait_for_function(check, 10, 500) {
        panic!(
            "created utxo number ({}) didn't match the expected one ({expected})",
            num.unwrap_or(UTXO_NUM)
        );
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_create_utxos_nowait(
    wallet: &mut Wallet,
    online: Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: u64,
) -> u8 {
    wallet
        .create_utxos(online, up_to, num, size, fee_rate, false)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_create_utxos_begin_result(
    wallet: &mut Wallet,
    online: Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: u64,
) -> Result<String, Error> {
    wallet.create_utxos_begin(online, up_to, num, size, fee_rate, false)
}

pub(crate) fn test_delete_transfers(
    wallet: &Wallet,
    batch_transfer_idx: Option<i32>,
    no_asset_only: bool,
) -> bool {
    test_delete_transfers_result(wallet, batch_transfer_idx, no_asset_only).unwrap()
}

pub(crate) fn test_delete_transfers_result(
    wallet: &Wallet,
    batch_transfer_idx: Option<i32>,
    no_asset_only: bool,
) -> Result<bool, Error> {
    wallet.delete_transfers(batch_transfer_idx, no_asset_only)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_drain_to_result(
    wallet: &mut Wallet,
    online: Online,
    address: &str,
    destroy_assets: bool,
) -> Result<String, Error> {
    wallet.drain_to(online, address.to_string(), destroy_assets, FEE_RATE)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_drain_to_begin_result(
    wallet: &mut Wallet,
    online: Online,
    address: &str,
    destroy_assets: bool,
    fee_rate: u64,
) -> Result<String, Error> {
    wallet.drain_to_begin(online, address.to_string(), destroy_assets, fee_rate)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_drain_to_destroy(wallet: &mut Wallet, online: Online, address: &str) -> String {
    wallet
        .drain_to(online, address.to_string(), true, FEE_RATE)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_drain_to_keep(wallet: &mut Wallet, online: Online, address: &str) -> String {
    wallet
        .drain_to(online, address.to_string(), false, FEE_RATE)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_fail_transfers_all(wallet: &mut Wallet, online: Online) -> bool {
    wallet.fail_transfers(online, None, false, false).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_fail_transfers_single(
    wallet: &mut Wallet,
    online: Online,
    batch_transfer_idx: i32,
) -> bool {
    wallet
        .fail_transfers(online, Some(batch_transfer_idx), false, false)
        .unwrap()
}

pub(crate) fn test_get_address(wallet: &mut Wallet) -> String {
    wallet.get_address().unwrap()
}

pub(crate) fn test_get_asset_balance(wallet: &impl RgbWalletOpsOffline, asset_id: &str) -> Balance {
    test_get_asset_balance_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_get_asset_balance_result(
    wallet: &impl RgbWalletOpsOffline,
    asset_id: &str,
) -> Result<Balance, Error> {
    wallet.get_asset_balance(asset_id.to_string())
}

pub(crate) fn test_get_asset_metadata(wallet: &Wallet, asset_id: &str) -> Metadata {
    test_get_asset_metadata_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_get_asset_metadata_result(
    wallet: &Wallet,
    asset_id: &str,
) -> Result<Metadata, Error> {
    wallet.get_asset_metadata(asset_id.to_string())
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_get_btc_balance(wallet: &mut Wallet, online: Online) -> BtcBalance {
    wallet.get_btc_balance(Some(online), false).unwrap()
}

pub(crate) fn test_get_wallet_data(wallet: &Wallet) -> WalletData {
    wallet.get_wallet_data()
}

pub(crate) fn test_get_wallet_dir(wallet: &impl RgbWalletOpsOffline) -> PathBuf {
    wallet.get_wallet_dir()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_go_online(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    indexer_url: Option<&str>,
) -> Online {
    test_go_online_result(wallet, skip_consistency_check, indexer_url).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_go_online_result(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    indexer_url: Option<&str>,
) -> Result<Online, Error> {
    let electrum = indexer_url.unwrap_or(ELECTRUM_URL).to_string();
    wallet.go_online(skip_consistency_check, electrum)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_inflate(
    wallet: &mut Wallet,
    online: Online,
    asset_id: &str,
    inflation_amounts: &[u64],
) -> OperationResult {
    test_inflate_result(wallet, online, asset_id, inflation_amounts).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_inflate_result(
    wallet: &mut Wallet,
    online: Online,
    asset_id: &str,
    inflation_amounts: &[u64],
) -> Result<OperationResult, Error> {
    wallet.inflate(
        online,
        asset_id.to_string(),
        inflation_amounts.to_vec(),
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_inflate_begin(
    wallet: &mut Wallet,
    online: Online,
    asset_id: &str,
    inflation_amounts: &[u64],
) -> String {
    test_inflate_begin_result(wallet, online, asset_id, inflation_amounts)
        .unwrap()
        .psbt
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_inflate_begin_result(
    wallet: &mut Wallet,
    online: Online,
    asset_id: &str,
    inflation_amounts: &[u64],
) -> Result<InflateBeginResult, Error> {
    wallet.inflate_begin(
        online,
        asset_id.to_string(),
        inflation_amounts.to_vec(),
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_inflate_end_result(
    wallet: &mut Wallet,
    online: Online,
    signed_psbt: &str,
) -> Result<OperationResult, Error> {
    wallet.inflate_end(online, signed_psbt.to_string())
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_uda(
    wallet: &mut Wallet,
    online: Online,
    details: Option<&str>,
    media_file_path: Option<&str>,
    attachments_file_paths: Vec<&str>,
) -> AssetUDA {
    test_issue_asset_uda_result(
        wallet,
        online,
        details,
        media_file_path,
        attachments_file_paths,
    )
    .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_uda_result(
    wallet: &mut Wallet,
    online: Online,
    details: Option<&str>,
    media_file_path: Option<&str>,
    attachments_file_paths: Vec<&str>,
) -> Result<AssetUDA, Error> {
    test_fail_transfers_all(wallet, online);
    wallet.issue_asset_uda(
        TICKER.to_string(),
        NAME.to_string(),
        details.map(|d| d.to_string()),
        PRECISION,
        media_file_path.map(|m| m.to_string()),
        attachments_file_paths
            .iter()
            .map(|a| a.to_string())
            .collect(),
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_cfa(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> AssetCFA {
    test_issue_asset_cfa_result(wallet, online, amounts, file_path).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_cfa_result(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> Result<AssetCFA, Error> {
    test_fail_transfers_all(wallet, online);
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_cfa(
        NAME.to_string(),
        Some(DETAILS.to_string()),
        PRECISION,
        amounts,
        file_path,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_nia(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
) -> AssetNIA {
    test_issue_asset_nia_result(wallet, online, amounts).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_nia_result(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
) -> Result<AssetNIA, Error> {
    test_fail_transfers_all(wallet, online);
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_nia(TICKER.to_string(), NAME.to_string(), PRECISION, amounts)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_ifa(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
    inflation_amounts: Option<&[u64]>,
    reject_list_url: Option<String>,
) -> AssetIFA {
    test_issue_asset_ifa_result(wallet, online, amounts, inflation_amounts, reject_list_url)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_issue_asset_ifa_result(
    wallet: &mut Wallet,
    online: Online,
    amounts: Option<&[u64]>,
    inflation_amounts: Option<&[u64]>,
    reject_list_url: Option<String>,
) -> Result<AssetIFA, Error> {
    test_fail_transfers_all(wallet, online);
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    let inflation_amounts = if let Some(a) = inflation_amounts {
        a.to_vec()
    } else {
        vec![AMOUNT_INFLATION]
    };
    wallet.issue_asset_ifa(
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        amounts,
        inflation_amounts,
        reject_list_url,
    )
}

pub(crate) fn test_list_assets(wallet: &Wallet, filter_asset_schemas: &[AssetSchema]) -> Assets {
    wallet.list_assets(filter_asset_schemas.to_vec()).unwrap()
}

pub(crate) fn test_list_transactions(
    wallet: &mut impl RgbWalletOpsOffline,
    online: Option<Online>,
) -> Vec<Transaction> {
    let skip_sync = online.is_none();
    wallet.list_transactions(online, skip_sync).unwrap()
}

pub(crate) fn test_list_transfers(
    wallet: &impl RgbWalletOpsOffline,
    asset_id: Option<&str>,
) -> Vec<Transfer> {
    test_list_transfers_result(wallet, asset_id).unwrap()
}

pub(crate) fn test_list_transfers_result(
    wallet: &impl RgbWalletOpsOffline,
    asset_id: Option<&str>,
) -> Result<Vec<Transfer>, Error> {
    let asset_id = asset_id.map(|a| a.to_string());
    wallet.list_transfers(asset_id)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_list_unspents(
    wallet: &mut impl RgbWalletOpsOnline,
    online: Option<Online>,
    settled_only: bool,
) -> Vec<Unspent> {
    let skip_sync = online.is_none();
    wallet
        .list_unspents(online, settled_only, skip_sync)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_list_unspents_vanilla(
    wallet: &mut Wallet,
    online: Online,
    min_confirmations: Option<u8>,
) -> Vec<LocalOutput> {
    let min_confirmations = min_confirmations.unwrap_or(MIN_CONFIRMATIONS);
    wallet
        .list_unspents_vanilla(online, min_confirmations, false)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_refresh_all(wallet: &mut Wallet, online: Online) -> bool {
    test_refresh_result(wallet, online, None, &[])
        .unwrap()
        .transfers_changed()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_refresh_asset(wallet: &mut Wallet, online: Online, asset_id: &str) -> bool {
    test_refresh_result(wallet, online, Some(asset_id), &[])
        .unwrap()
        .transfers_changed()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_refresh_result(
    wallet: &mut impl RgbWalletOpsOnline,
    online: Online,
    asset_id: Option<&str>,
    filter: &[RefreshFilter],
) -> Result<RefreshResult, Error> {
    wallet.refresh(
        online,
        asset_id.map(|a| a.to_string()),
        filter.to_vec(),
        false,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_save_new_asset(
    wallet: &mut Wallet,
    online: Online,
    rcv_wallet: &mut Wallet,
    asset_id: &String,
    assignment: Assignment,
) {
    let receive_data = test_witness_receive(rcv_wallet);
    let recipient_map = HashMap::from([(
        asset_id.to_owned(),
        vec![Recipient {
            assignment,
            recipient_id: receive_data.recipient_id.clone(),
            witness_data: Some(WitnessData {
                amount_sat: 1000,
                blinding: None,
            }),
            transport_endpoints: TRANSPORT_ENDPOINTS.clone(),
        }],
    )]);
    let txid = test_send(wallet, online, &recipient_map);
    assert!(!txid.is_empty());

    let consignment_path = wallet.get_send_consignment_path(asset_id, &txid);
    let consignment = RgbTransfer::load_file(consignment_path).unwrap();

    let contract = consignment.clone().into_contract();
    let asset_schema: AssetSchema = consignment.schema_id().try_into().unwrap();
    let validation_config = ValidationConfig {
        chain_net: rcv_wallet.chain_net(),
        trusted_typesystem: asset_schema.types(),
        ..Default::default()
    };
    let mut runtime = rcv_wallet.rgb_runtime().unwrap();
    let valid_contract = contract
        .clone()
        .validate(&DumbResolver, &validation_config)
        .unwrap();
    runtime
        .import_contract(valid_contract, rcv_wallet.blockchain_resolver())
        .unwrap();
    drop(runtime);

    rcv_wallet.save_new_asset(consignment, txid).unwrap();
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_send(
    wallet: &mut Wallet,
    online: Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> String {
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    loop {
        if start.elapsed() > timeout {
            panic!("send failed")
        }
        let result = test_send_result(wallet, online, recipient_map);
        if let Err(e) = result {
            println!("send error: {e}");
            std::thread::sleep(Duration::from_millis(500));
            wallet.sync(online).unwrap();
            continue;
        }
        break result.unwrap().txid;
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_send_result(
    wallet: &mut Wallet,
    online: Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<OperationResult, Error> {
    wallet.send(
        online,
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
        Some((now().unix_timestamp() + DURATION_SEND_TRANSFER as i64) as u64),
        false,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_send_begin_result(
    wallet: &mut Wallet,
    online: Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<String, Error> {
    wallet
        .send_begin(
            online,
            recipient_map.clone(),
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
            None,
        )
        .map(|r| r.psbt)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_send_btc(
    wallet: &mut Wallet,
    online: Online,
    address: &str,
    amount: u64,
) -> String {
    test_send_btc_result(wallet, online, address, amount).unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn test_send_btc_result(
    wallet: &mut Wallet,
    online: Online,
    address: &str,
    amount: u64,
) -> Result<String, Error> {
    wallet.send_btc(online, address.to_string(), amount, FEE_RATE, false)
}
