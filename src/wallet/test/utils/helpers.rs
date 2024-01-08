use super::*;

pub(crate) fn join_with_sep(parts: &[&str]) -> String {
    parts.join(&MAIN_SEPARATOR.to_string())
}

pub(crate) fn get_restore_dir_string() -> String {
    join_with_sep(&RESTORE_DIR_PARTS)
}

pub(crate) fn get_test_data_dir_string() -> String {
    join_with_sep(&TEST_DATA_DIR_PARTS)
}

pub(crate) fn get_restore_dir_path() -> PathBuf {
    PathBuf::from(get_restore_dir_string())
}

pub(crate) fn get_test_data_dir_path() -> PathBuf {
    PathBuf::from(get_test_data_dir_string())
}

pub(crate) fn get_test_wallet_data(data_dir: &str, pubkey: &str, mnemonic: &str) -> WalletData {
    WalletData {
        data_dir: data_dir.to_string(),
        bitcoin_network: BitcoinNetwork::Regtest,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: MAX_ALLOCATIONS_PER_UTXO,
        pubkey: pubkey.to_string(),
        mnemonic: Some(mnemonic.to_string()),
        vanilla_keychain: None,
    }
}

// return a wallet for testing
pub(crate) fn get_test_wallet_with_net(
    private_keys: bool,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    fs::create_dir_all(get_test_data_dir_path()).unwrap();

    let keys = generate_keys(bitcoin_network);
    let mut mnemonic = None;
    if private_keys {
        mnemonic = Some(keys.mnemonic)
    }
    Wallet::new(WalletData {
        data_dir: get_test_data_dir_string(),
        bitcoin_network,
        database_type: DatabaseType::Sqlite,
        max_allocations_per_utxo: max_allocations_per_utxo.unwrap_or(MAX_ALLOCATIONS_PER_UTXO),
        pubkey: keys.xpub,
        mnemonic,
        vanilla_keychain: None,
    })
    .unwrap()
}

// return a regtest wallet for testing
pub(crate) fn get_test_wallet(private_keys: bool, max_allocations_per_utxo: Option<u32>) -> Wallet {
    get_test_wallet_with_net(
        private_keys,
        max_allocations_per_utxo,
        BitcoinNetwork::Regtest,
    )
}

pub(crate) fn get_empty_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys, None);
    if print_log {
        println!("wallet directory: {:?}", test_get_wallet_dir(&wallet));
    }
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();
    (wallet, online)
}

pub(crate) fn get_funded_noutxo_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let (wallet, online) = get_empty_wallet(print_log, private_keys);
    fund_wallet(wallet.get_address().unwrap());
    (wallet, online)
}

pub(crate) fn get_funded_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let (wallet, online) = get_funded_noutxo_wallet(print_log, private_keys);
    test_create_utxos_default(&wallet, &online);
    (wallet, online)
}

pub(crate) fn drain_wallet(wallet: &Wallet, online: &Online) {
    let rcv_wallet = get_test_wallet(false, None);
    test_drain_to_destroy(wallet, online, &rcv_wallet.get_address().unwrap());
}

pub(crate) fn fund_wallet(address: String) {
    let status = Command::new("docker")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("compose")
        .args(_bitcoin_cli())
        .arg("-rpcwallet=miner")
        .arg("sendtoaddress")
        .arg(address)
        .arg("1")
        .status()
        .expect("failed to fund wallet");
    assert!(status.success());
}

pub(crate) fn check_test_transfer_status_recipient(
    wallet: &Wallet,
    recipient_id: &str,
    expected_status: TransferStatus,
) -> bool {
    let transfers = wallet.database.iter_transfers().unwrap();
    let transfer = transfers
        .iter()
        .find(|t| t.recipient_id == Some(recipient_id.to_string()))
        .unwrap();
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

pub(crate) fn check_test_wallet_data(
    wallet: &Wallet,
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
    assert_eq!(metadata.asset_iface, AssetIface::RGB20);
    assert_eq!(metadata.issued_supply, issued_supply);
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
        .database
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
        .database
        .iter_asset_transfers()
        .unwrap()
        .into_iter()
        .filter(|at| at.batch_transfer_idx == batch_transfer_idx)
        .collect()
}

pub(crate) fn get_test_transfers(wallet: &Wallet, asset_transfer_idx: i32) -> Vec<DbTransfer> {
    wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .filter(|t| t.asset_transfer_idx == asset_transfer_idx)
        .collect()
}

pub(crate) fn get_test_asset_transfer(wallet: &Wallet, batch_transfer_idx: i32) -> DbAssetTransfer {
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer_idx);
    assert_eq!(asset_transfers.len(), 1);
    asset_transfers.first().unwrap().clone()
}

pub(crate) fn get_test_coloring(wallet: &Wallet, asset_transfer_idx: i32) -> DbColoring {
    let colorings: Vec<DbColoring> = wallet
        .database
        .iter_colorings()
        .unwrap()
        .into_iter()
        .filter(|c| c.asset_transfer_idx == asset_transfer_idx)
        .collect();
    assert_eq!(colorings.len(), 1);
    colorings.first().unwrap().clone()
}

pub(crate) fn get_test_transfer_recipient(wallet: &Wallet, recipient_id: &str) -> DbTransfer {
    wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .find(|t| t.recipient_id == Some(recipient_id.to_string()))
        .unwrap()
}

pub(crate) fn get_test_transfer_sender(
    wallet: &Wallet,
    txid: &str,
) -> (DbTransfer, DbAssetTransfer, DbBatchTransfer) {
    let batch_transfers = get_test_batch_transfers(wallet, txid);
    assert_eq!(batch_transfers.len(), 1);
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfer = get_test_asset_transfer(wallet, batch_transfer.idx);
    let transfers = get_test_transfers(wallet, asset_transfer.idx);
    assert_eq!(transfers.len(), 1);
    let transfer = transfers.first().unwrap();
    (transfer.clone(), asset_transfer, batch_transfer.clone())
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
    let batch_transfer = batch_transfers.first().unwrap();
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer.idx);
    let mut transfers: HashMap<String, Vec<DbTransfer>> = HashMap::new();
    for asset_transfer in asset_transfers.clone() {
        let asset_id = asset_transfer.asset_id.unwrap();
        let transfers_for_asset = get_test_transfers(wallet, asset_transfer.idx);
        transfers.insert(asset_id, transfers_for_asset);
    }
    (transfers.clone(), asset_transfers, batch_transfer.clone())
}

pub(crate) fn get_test_transfer_data(
    wallet: &Wallet,
    transfer: &DbTransfer,
) -> (TransferData, DbAssetTransfer) {
    let db_data = wallet.database.get_db_data(false).unwrap();
    let (asset_transfer, batch_transfer) = transfer
        .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
        .unwrap();
    let transfer_data = wallet
        .database
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
    let db_data = wallet.database.get_db_data(false).unwrap();
    transfer
        .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
        .unwrap()
}

pub(crate) fn get_test_txo(wallet: &Wallet, idx: i32) -> DbTxo {
    wallet
        .database
        .iter_txos()
        .unwrap()
        .into_iter()
        .find(|t| t.idx == idx)
        .unwrap()
}

pub(crate) fn list_test_unspents(wallet: &Wallet, msg: &str) -> Vec<Unspent> {
    let unspents = test_list_unspents(wallet, None, false);
    println!(
        "unspents for wallet {:?} {}: {}",
        test_get_wallet_dir(wallet),
        msg,
        unspents.len()
    );
    for u in &unspents {
        println!(
            "- {:?} {:?} {:?}",
            u.utxo.outpoint, u.utxo.btc_amount, u.utxo.colorable
        );
        for a in &u.rgb_allocations {
            println!("  - {:?} {:?} {:?}", a.asset_id, a.amount, a.settled);
        }
    }
    unspents
}

pub(crate) fn wait_for_btc_balance(
    wallet: &Wallet,
    online: &Online,
    expected_balance: &BtcBalance,
) {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let current_balance = test_get_btc_balance(wallet, online);
        if &current_balance == expected_balance {
            break;
        }
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 10.0 {
            println!("current balance: {current_balance:?}");
            println!("expected balance: {expected_balance:?}");
            panic!("BTC balance is not becoming the expected one");
        }
    }
}

/// print the provided message, then get colorings for each wallet unspent and print their status,
/// type, amount and asset
pub(crate) fn show_unspent_colorings(wallet: &Wallet, msg: &str) {
    println!("\n{msg}");
    let unspents: Vec<Unspent> = test_list_unspents(wallet, None, false)
        .into_iter()
        .filter(|u| u.utxo.colorable)
        .collect();
    for unspent in unspents {
        let outpoint = unspent.utxo.outpoint;
        let db_txos = wallet.database.iter_txos().unwrap();
        let db_txo = db_txos
            .iter()
            .find(|t| t.txid == outpoint.txid && t.vout == outpoint.vout)
            .unwrap();
        let db_colorings: Vec<DbColoring> = wallet
            .database
            .iter_colorings()
            .unwrap()
            .into_iter()
            .filter(|c| c.txo_idx == db_txo.idx)
            .collect();
        println!(
            "> {}:{}, {} sat",
            outpoint.txid, outpoint.vout, unspent.utxo.btc_amount,
        );
        for db_coloring in db_colorings {
            let db_asset_transfers = wallet.database.iter_asset_transfers().unwrap();
            let db_asset_transfer = db_asset_transfers
                .iter()
                .find(|a| a.idx == db_coloring.asset_transfer_idx)
                .unwrap();
            let db_batch_transfers = wallet.database.iter_batch_transfers().unwrap();
            let db_batch_transfer = db_batch_transfers
                .iter()
                .find(|b| b.idx == db_asset_transfer.batch_transfer_idx)
                .unwrap();
            println!(
                "\t- {:?} {:?} of {:?} for {:?}",
                db_batch_transfer.status,
                db_coloring.r#type,
                db_coloring.amount,
                db_asset_transfer.asset_id.as_ref(),
            );
        }
    }
}
