use super::*;

/// Panic if the given expression doesn't match the provided pattern, logging the unexpected result
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $pattern:pat $(if $guard:expr)? $(,)?) => {
        match $expression {
            $pattern $(if $guard)? => {},
            _ => {
                panic!("received unexpected result: {}", format!("{:?}", $expression));
            }
        }
    };
}

pub(crate) fn join_with_sep(parts: &[&str]) -> String {
    parts.join(MAIN_SEPARATOR_STR)
}

pub(crate) fn get_current_time() -> u128 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub(crate) fn get_restore_dir_string() -> String {
    join_with_sep(&RESTORE_DIR_PARTS)
}

pub(crate) fn get_test_data_dir_string() -> String {
    join_with_sep(&TEST_DATA_DIR_PARTS)
}

pub(crate) fn get_restore_dir_path<P: AsRef<Path>>(last: Option<P>) -> PathBuf {
    let mut path = PathBuf::from(get_restore_dir_string());
    if let Some(l) = last {
        path = path.join(l);
    }
    path
}

pub(crate) fn get_test_data_dir_path() -> PathBuf {
    PathBuf::from(get_test_data_dir_string())
}

pub(crate) fn create_test_data_dir() -> PathBuf {
    let test_data_dir = get_test_data_dir_path();
    if !test_data_dir.exists() {
        fs::create_dir_all(&test_data_dir).unwrap();
    }
    test_data_dir
}

pub(crate) fn get_test_wallet_data(data_dir: &str) -> WalletData {
    WalletData {
        data_dir: data_dir.to_string(),
        bitcoin_network: BitcoinNetwork::Regtest,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        supported_schemas: AssetSchema::VALUES.to_vec(),
    }
}

pub(crate) fn get_test_wallet_with_keys(keys: &Keys) -> Wallet {
    let wallet_keys = SinglesigKeys::from_keys(keys, None);
    get_test_wallet_raw(&wallet_keys, None, BitcoinNetwork::Regtest)
}

// return a wallet for testing
pub(crate) fn get_test_wallet_with_net(
    private_keys: bool,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    let keys = generate_keys(bitcoin_network);
    let wallet_keys = if private_keys {
        SinglesigKeys::from_keys(&keys, None)
    } else {
        SinglesigKeys::from_keys_no_mnemonic(&keys, None)
    };
    get_test_wallet_raw(&wallet_keys, max_allocations_per_utxo, bitcoin_network)
}

// return a wallet for testing
pub(crate) fn get_test_wallet_raw(
    wallet_keys: &SinglesigKeys,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    create_test_data_dir();

    let wallet = Wallet::new(
        WalletData {
            data_dir: get_test_data_dir_string(),
            bitcoin_network,
            database_type: DatabaseType::Sqlite,
            max_allocations_per_utxo: max_allocations_per_utxo.unwrap_or(MAX_ALLOCATIONS_PER_UTXO),
            supported_schemas: AssetSchema::VALUES.to_vec(),
        },
        wallet_keys.clone(),
    )
    .unwrap();
    println!("wallet directory: {:?}", test_get_wallet_dir(&wallet));
    wallet
}

// return a regtest wallet for testing
pub(crate) fn get_test_wallet(private_keys: bool, max_allocations_per_utxo: Option<u32>) -> Wallet {
    get_test_wallet_with_net(
        private_keys,
        max_allocations_per_utxo,
        BitcoinNetwork::Regtest,
    )
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_empty_wallet(
    private_keys: bool,
    indexer_url: Option<String>,
) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys, None);
    let online = wallet
        .go_online(true, indexer_url.unwrap_or(ELECTRUM_URL.to_string()))
        .unwrap();
    (wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_noutxo_wallet(
    private_keys: bool,
    indexer_url: Option<String>,
) -> (Wallet, Online) {
    let (mut wallet, online) = get_empty_wallet(private_keys, indexer_url);
    fund_wallet(wallet.get_address().unwrap());
    (wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_funded_wallet(
    private_keys: bool,
    indexer_url: Option<String>,
) -> (Wallet, Online) {
    let (mut wallet, online) = get_funded_noutxo_wallet(private_keys, indexer_url);
    test_create_utxos_default(&mut wallet, online);
    (wallet, online)
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn drain_wallet(wallet: &mut Wallet, online: Online) {
    let mut rcv_wallet = get_test_wallet(false, None);
    test_drain_to_destroy(wallet, online, &rcv_wallet.get_address().unwrap());
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn send_to_address(address: String) {
    send_sats_to_address(address, None);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn send_sats_to_address(address: String, sats: Option<u64>) {
    let amt = BdkAmount::from_sat(sats.unwrap_or(100_000_000));
    let btc_str = amt.to_string_in(Denomination::Bitcoin);
    let t_0 = OffsetDateTime::now_utc();
    let bitcoin_cli = bitcoin_cli();
    loop {
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            panic!("could not send to address ({QUEUE_DEPTH_EXCEEDED})");
        }
        let output = Command::new("docker")
            .stdin(Stdio::null())
            .arg("compose")
            .args(&bitcoin_cli)
            .arg("-rpcwallet=miner")
            .arg("sendtoaddress")
            .arg(&address)
            .arg(&btc_str)
            .output()
            .expect("failed to fund wallet");
        if !output.status.success()
            && String::from_utf8(output.stderr)
                .unwrap()
                .contains(QUEUE_DEPTH_EXCEEDED)
        {
            eprintln!("work queue depth exceeded");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }
        assert!(output.status.success());
        break;
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn fund_wallet(address: String) {
    send_to_address(address);
    mine(false, false);
}

pub(crate) fn check_test_transfer_status_recipient(
    wallet: &Wallet,
    recipient_id: &str,
    expected_status: TransferStatus,
) -> bool {
    let transfers = wallet.database().iter_transfers().unwrap();
    let mut recipient_transfers = transfers
        .iter()
        .filter(|t| t.recipient_id.as_deref() == Some(recipient_id));
    let transfer = recipient_transfers.next().unwrap();
    assert!(recipient_transfers.next().is_none());
    let (transfer_data, _) = get_test_transfer_data(wallet, transfer);
    println!(
        "receive with recipient_id {} is in status {:?}",
        recipient_id, &transfer_data.status
    );
    transfer_data.status == expected_status
}

pub(crate) fn check_test_transfer_status_sender(
    wallet: &Wallet,
    txid: &str,
    expected_status: TransferStatus,
) -> bool {
    let batch_transfers = get_test_batch_transfers(wallet, txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    println!(
        "send with txid {} is in status {:?}",
        txid, &batch_transfer.status
    );
    batch_transfer.status == expected_status
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn check_test_wallet_data(
    wallet: &mut Wallet,
    asset: &AssetNIA,
    custom_issued_supply: Option<u64>,
    transfer_num: usize,
    spent_amount: u64,
) {
    println!("checking wallet data...");
    let issued_supply = match custom_issued_supply {
        Some(supply) => supply,
        None => AMOUNT,
    };
    // asset list
    let assets = test_list_assets(wallet, &[]);
    let nia_assets = assets.nia.unwrap();
    let cfa_assets = assets.cfa.unwrap();
    assert_eq!(nia_assets.len(), 1);
    assert_eq!(cfa_assets.len(), 0);
    let nia_asset = nia_assets.first().unwrap();
    assert_eq!(nia_asset.asset_id, asset.asset_id);
    // asset balance
    let balance = wallet.get_asset_balance(asset.asset_id.clone()).unwrap();
    assert_eq!(
        balance,
        Balance {
            settled: asset.balance.settled - spent_amount,
            future: asset.balance.future - spent_amount,
            spendable: asset.balance.spendable - spent_amount,
        }
    );
    // asset metadata
    let metadata = test_get_asset_metadata(wallet, &asset.asset_id);
    assert_eq!(metadata.asset_schema, AssetSchema::Nia);
    assert_eq!(metadata.initial_supply, issued_supply);
    assert_eq!(metadata.name, asset.name);
    assert_eq!(metadata.precision, asset.precision);
    assert_eq!(metadata.ticker.unwrap(), asset.ticker);
    // transfer list
    let transfers = test_list_transfers(wallet, Some(&asset.asset_id));
    assert_eq!(transfers.len(), 1 + transfer_num);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    assert_eq!(transfers.last().unwrap().kind, TransferKind::Send);
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    // unspent list
    let unspents = test_list_unspents(wallet, None, false);
    assert_eq!(unspents.len(), 6);
}

pub(crate) fn compare_test_directories(src: &Path, dst: &Path, skip: &[&str]) {
    let ignores = RegexSet::new(skip).unwrap();
    let cmp = dircmp::Comparison::new(ignores);
    let diff = cmp.compare(src, dst).unwrap();
    assert!(diff.is_empty());
}

pub(crate) fn get_test_batch_transfers(wallet: &Wallet, txid: &str) -> Vec<DbBatchTransfer> {
    wallet
        .database()
        .iter_batch_transfers()
        .unwrap()
        .into_iter()
        .filter(|b| b.txid == Some(txid.to_string()))
        .collect()
}

pub(crate) fn get_test_asset_transfers(
    wallet: &Wallet,
    batch_transfer_idx: i32,
) -> Vec<DbAssetTransfer> {
    wallet
        .database()
        .iter_asset_transfers()
        .unwrap()
        .into_iter()
        .filter(|at| at.batch_transfer_idx == batch_transfer_idx)
        .collect()
}

pub(crate) fn get_test_transfers(
    wallet: &Wallet,
    asset_transfer_idx: i32,
) -> impl Iterator<Item = DbTransfer> {
    wallet
        .database()
        .iter_transfers()
        .unwrap()
        .into_iter()
        .filter(move |t| t.asset_transfer_idx == asset_transfer_idx)
}

pub(crate) fn get_test_asset_transfer(wallet: &Wallet, batch_transfer_idx: i32) -> DbAssetTransfer {
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer_idx);
    let mut user_driven_transfers = asset_transfers.into_iter().filter(|t| t.user_driven);
    let user_driven_transfer = user_driven_transfers.next().unwrap();
    assert!(user_driven_transfers.next().is_none());
    user_driven_transfer
}

pub(crate) fn get_test_colorings(wallet: &Wallet, asset_transfer_idx: i32) -> Vec<DbColoring> {
    wallet
        .database()
        .iter_colorings()
        .unwrap()
        .into_iter()
        .filter(|c| c.asset_transfer_idx == asset_transfer_idx)
        .collect()
}

pub(crate) fn get_test_transfer_recipient(wallet: &Wallet, recipient_id: &str) -> DbTransfer {
    let mut transfers = wallet
        .database()
        .iter_transfers()
        .unwrap()
        .into_iter()
        .filter(|t| t.recipient_id == Some(recipient_id.to_string()) && t.incoming);
    let transfer = transfers.next().unwrap();
    assert!(transfers.next().is_none());
    transfer
}

pub(crate) fn get_test_transfer_sender(
    wallet: &Wallet,
    txid: &str,
) -> (DbTransfer, DbAssetTransfer, DbBatchTransfer) {
    let batch_transfers = get_test_batch_transfers(wallet, txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.into_iter().next().unwrap();
    let asset_transfer = get_test_asset_transfer(wallet, batch_transfer.idx);
    let mut transfers = get_test_transfers(wallet, asset_transfer.idx);
    let transfer = transfers.next().unwrap();
    assert!(transfers.next().is_none());
    (transfer, asset_transfer, batch_transfer)
}

pub(crate) fn get_test_transfers_sender(
    wallet: &Wallet,
    txid: &str,
) -> (
    HashMap<String, Vec<DbTransfer>>,
    Vec<DbAssetTransfer>,
    DbBatchTransfer,
) {
    let batch_transfers = get_test_batch_transfers(wallet, txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.into_iter().next().unwrap();
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer.idx);
    let mut transfers: HashMap<String, Vec<DbTransfer>> = HashMap::new();
    for asset_transfer in &asset_transfers {
        let asset_id = asset_transfer.asset_id.clone().unwrap();
        let transfers_for_asset = get_test_transfers(wallet, asset_transfer.idx);
        transfers.insert(asset_id, transfers_for_asset.collect());
    }
    (transfers, asset_transfers, batch_transfer)
}

pub(crate) fn get_test_transfer_data(
    wallet: &Wallet,
    transfer: &DbTransfer,
) -> (TransferData, DbAssetTransfer) {
    let db_data = wallet.database().get_db_data(false).unwrap();
    let (asset_transfer, batch_transfer) = transfer
        .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
        .unwrap();
    let transfer_data = wallet
        .get_transfer_data(
            transfer,
            &asset_transfer,
            &batch_transfer,
            &db_data.txos,
            &db_data.colorings,
        )
        .unwrap();
    (transfer_data, asset_transfer)
}

pub(crate) fn get_test_transfer_related(
    wallet: &Wallet,
    transfer: &DbTransfer,
) -> (DbAssetTransfer, DbBatchTransfer) {
    let db_data = wallet.database().get_db_data(false).unwrap();
    transfer
        .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
        .unwrap()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn list_test_unspents(wallet: &mut Wallet, msg: &str) -> Vec<Unspent> {
    let unspents = test_list_unspents(wallet, None, false);
    print_unspents(&unspents, msg);
    unspents
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_colorable_unspents(
    wallet: &mut Wallet,
    online: Option<Online>,
    settled_only: bool,
) -> Vec<Unspent> {
    test_list_unspents(wallet, online, settled_only)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect()
}

pub(crate) fn print_unspents(unspents: &[Unspent], msg: &str) {
    println!("\n{msg} ({} unspents)", unspents.len());
    for u in unspents {
        println!(
            "> {} {} {}",
            u.utxo.outpoint,
            u.utxo.btc_amount,
            if u.utxo.colorable {
                "colorable"
            } else {
                "vanilla"
            }
        );
        for a in &u.rgb_allocations {
            println!(
                "\t- {} {:?} {}",
                a.asset_id.as_ref().unwrap(),
                a.assignment,
                if a.settled { "settled" } else { "pending" }
            )
        }
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn wait_for_asset_balance(wallet: &Wallet, asset_id: &str, expected_balance: &Balance) {
    println!("waiting for asset balance");
    let mut current_balance = test_get_asset_balance(wallet, asset_id);
    let check = || {
        current_balance = test_get_asset_balance(wallet, asset_id);
        if &current_balance == expected_balance {
            return true;
        }
        false
    };
    if !wait_for_function(check, 10, 500) {
        println!("current balance: {current_balance:?}");
        println!("expected balance: {expected_balance:?}");
        panic!("asset balance is not becoming the expected one");
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn wait_for_btc_balance(
    wallet: &mut Wallet,
    online: Online,
    expected_balance: &BtcBalance,
) {
    println!("waiting for BTC balance");
    let mut current_balance = test_get_btc_balance(wallet, online);
    let check = || {
        current_balance = test_get_btc_balance(wallet, online);
        if &current_balance == expected_balance {
            return true;
        }
        false
    };
    if !wait_for_function(check, 10, 500) {
        println!("current balance: {current_balance:?}");
        println!("expected balance: {expected_balance:?}");
        panic!("BTC balance is not becoming the expected one");
    }
}

pub(crate) fn wait_for_function<F>(mut func: F, timeout_secs: u8, interval_ms: u16) -> bool
where
    F: FnMut() -> bool,
{
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs as u64);
    while start.elapsed() < timeout {
        if func() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(interval_ms as u64));
    }
    false
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn wait_for_refresh(
    wallet: &mut impl RgbWalletOpsOnline,
    online: Online,
    asset_id: Option<&str>,
    transfer_ids: Option<&[i32]>,
) {
    println!(
        "waiting for refresh ({})",
        wallet.internals().wallet_data.data_dir
    );
    let mut seen = HashSet::new();
    let mut target_set = HashSet::new();
    if let Some(t_ids) = transfer_ids {
        assert!(!t_ids.is_empty());
        target_set = t_ids.iter().copied().collect();
    }
    let check = || {
        let result = test_refresh_result(wallet, online, asset_id, &[]);
        if let Ok(refresh_res) = result {
            let mut non_fatal_error = false;
            refresh_res.iter().for_each(|(i, rt)| {
                if let Some(ref e) = rt.failure {
                    eprintln!("refresh of {i} failure: {e} ({e:?})");
                    match e {
                        Error::Internal { details } => {
                            println!("refresh of {i} internal error: {e}, details: {details}");
                            non_fatal_error = true;
                        }
                        Error::InvalidTxid => {
                            println!("refresh of {i} invalid TXID: {e}");
                            non_fatal_error = true;
                        }
                        Error::Network { details } => {
                            println!("refresh of {i} network error: {e}, details: {details}");
                            non_fatal_error = true;
                        }
                        _ => panic!("refresh of {i} fatal error: {e}"),
                    }
                }
            });
            if non_fatal_error {
                return false;
            }
            if transfer_ids.is_some() {
                for (id, rt) in refresh_res {
                    if rt.updated_status.is_some() && target_set.contains(&id) {
                        seen.insert(id);
                    }
                }
                if seen == target_set {
                    return true;
                }
            } else if refresh_res.transfers_changed() {
                return true;
            }
        } else {
            eprintln!("refresh error: {result:?}");
            return false;
        };
        false
    };
    if !wait_for_function(check, 10, 500) {
        panic!("transfer(s) are not refreshing");
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn wait_for_unspents(
    wallet: &mut Wallet,
    online: Option<Online>,
    settled_only: bool,
    expected_len: u8,
) {
    println!("waiting for unspents");
    let mut unspents = test_list_unspents(wallet, online, settled_only);
    let check = || {
        unspents = test_list_unspents(wallet, online, settled_only);
        unspents.len() == expected_len as usize
    };
    if !wait_for_function(check, 10, 500) {
        panic!(
            "UTXO num {} is not becoming the expected {expected_len}",
            unspents.len()
        );
    }
}

pub(crate) fn get_pending_blind_transfers(wallet: &mut impl RgbWalletOpsOffline) -> Vec<Transfer> {
    let transfers = test_list_transfers(wallet, None);
    transfers
        .into_iter()
        .filter(|t| t.status.pending() && t.kind == TransferKind::ReceiveBlind)
        .collect()
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn extract_opouts_from_transfer(
    wallet: &Wallet,
    asset_id: &str,
    txid: &str,
) -> Vec<Opout> {
    let batch_transfers = get_test_batch_transfers(wallet, txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer.idx);
    let asset_transfers = asset_transfers
        .iter()
        .filter(|at| at.asset_id.as_ref() == Some(&asset_id.to_string()))
        .filter(|t| t.user_driven)
        .collect::<Vec<_>>();
    assert_eq!(asset_transfers.len(), 1);
    let asset_transfer = asset_transfers.first().unwrap();
    let colorings: Vec<DbColoring> = wallet
        .database()
        .iter_colorings()
        .unwrap()
        .into_iter()
        .filter(|c| c.asset_transfer_idx == asset_transfer.idx)
        .collect();
    if colorings.is_empty() {
        panic!("cannot find colorings for this transfer");
    }
    let txo_indices = colorings.iter().map(|c| c.txo_idx).collect::<Vec<_>>();
    let db_txos = wallet.database().iter_txos().unwrap();
    let relevant_txos = db_txos.into_iter().filter(|t| txo_indices.contains(&t.idx));
    let mut outpoints = relevant_txos
        .map(|txo| OutPoint::from(txo.clone()))
        .peekable();
    if outpoints.peek().is_none() {
        panic!("cannot find outpoints for this transfer");
    }
    let contract_id = ContractId::from_str(asset_id).unwrap();
    let runtime = wallet.rgb_runtime().unwrap();
    let assignments = runtime
        .contract_assignments_for(contract_id, outpoints)
        .unwrap();
    let mut opouts = Vec::new();
    for (_explicit_seal, opout_state_map) in assignments {
        for (opout, _state) in opout_state_map {
            opouts.push(opout);
        }
    }
    opouts
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn write_opouts_to_reject_list(filename: &str, opouts: &[String]) {
    let lists_dir = PathBuf::from(join_with_sep(&LISTS_DIR_PARTS));
    if !lists_dir.exists() {
        fs::create_dir_all(&lists_dir).unwrap();
    }
    let file_path = lists_dir.join(filename);
    let mut file = std::fs::File::create(&file_path).unwrap();
    for opout in opouts {
        writeln!(file, "{}", opout).unwrap()
    }
}

/// print the provided message, then get colorings for each wallet unspent and print their status,
/// type, amount and asset
#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn show_unspent_colorings(wallet: &mut impl RgbWalletOpsOnline, msg: &str) {
    println!(
        "\nwallet {} unspent colorings ({msg})",
        wallet.get_wallet_data().data_dir
    );
    let unspents = test_list_unspents(wallet, None, false)
        .into_iter()
        .filter(|u| u.utxo.colorable);
    let db_txos = wallet.database().iter_txos().unwrap();
    let db_colorings = wallet.database().iter_colorings().unwrap();
    let db_asset_transfers = wallet.database().iter_asset_transfers().unwrap();
    let db_batch_transfers = wallet.database().iter_batch_transfers().unwrap();
    let pending_blind_transfers = get_pending_blind_transfers(wallet);
    for unspent in unspents {
        let outpoint = unspent.utxo.outpoint;
        let db_txo = db_txos
            .iter()
            .find(|t| t.txid == outpoint.txid && t.vout == outpoint.vout)
            .unwrap();
        let txo_pending_blind_transfers = pending_blind_transfers.iter().filter(|t| {
            if let Some(txo) = &t.receive_utxo {
                db_txo.outpoint() == *txo
            } else {
                false
            }
        });
        println!(
            "> {}:{}, {} sat{}",
            outpoint.txid,
            outpoint.vout,
            unspent.utxo.btc_amount,
            if !unspent.utxo.exists {
                " - tx not broadcast yet"
            } else {
                ""
            },
        );
        let txo_db_colorings = db_colorings.iter().filter(|c| c.txo_idx == db_txo.idx);
        for db_coloring in txo_db_colorings {
            let db_asset_transfer = db_asset_transfers
                .iter()
                .find(|a| a.idx == db_coloring.asset_transfer_idx)
                .unwrap();
            let db_batch_transfer = db_batch_transfers
                .iter()
                .find(|b| b.idx == db_asset_transfer.batch_transfer_idx)
                .unwrap();
            println!(
                "\t- {:?} {:?} of {:?} for {:?}",
                db_batch_transfer.status,
                db_coloring.r#type,
                db_coloring.assignment,
                db_asset_transfer.asset_id.as_ref(),
            );
        }
        for pbt in txo_pending_blind_transfers {
            println!("\t- pending blind receive with transfer ID {}", pbt.idx);
        }
    }
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub(crate) fn get_proxy_client(proxy_url: Option<&str>) -> ProxyClient {
    ProxyClient::new(proxy_url.unwrap_or(PROXY_URL)).unwrap()
}
