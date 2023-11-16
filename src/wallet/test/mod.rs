use amplify::s;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use regex::RegexSet;
use std::process::{Command, Stdio};
use std::sync::{Mutex, Once, RwLock};
use time::OffsetDateTime;

use crate::generate_keys;

use super::*;

const PROXY_HOST: &str = "127.0.0.1:3000/json-rpc";
const PROXY_HOST_MOD_API: &str = "127.0.0.1:3002/json-rpc";
const PROXY_HOST_MOD_PROTO: &str = "127.0.0.1:3001/json-rpc";
const PROXY_URL: &str = "http://127.0.0.1:3000/json-rpc";
const PROXY_URL_MOD_API: &str = "http://127.0.0.1:3002/json-rpc";
const PROXY_URL_MOD_PROTO: &str = "http://127.0.0.1:3001/json-rpc";
static PROXY_ENDPOINT: Lazy<String> = Lazy::new(|| format!("rpc://{PROXY_HOST}"));
static TRANSPORT_ENDPOINTS: Lazy<Vec<String>> = Lazy::new(|| vec![PROXY_ENDPOINT.clone()]);
const ELECTRUM_URL: &str = "127.0.0.1:50001";
const ELECTRUM_2_URL: &str = "127.0.0.1:50002";
const TEST_DATA_DIR: &str = "./tests/tmp";
const TICKER: &str = "TICKER";
const NAME: &str = "asset name";
const DESCRIPTION: &str = "description with ℧nicode characters";
const PRECISION: u8 = 7;
const AMOUNT: u64 = 666;
const FEE_RATE: f32 = 1.5;
const FEE_MSG_LOW: &str = "value under minimum 1";
const FEE_MSG_HIGH: &str = "value above maximum 1000";
const IDENT_EMPTY_MSG: &str = "ident must contain at least one character";
const IDENT_TOO_LONG_MSG: &str = "identifier name has invalid length";
const IDENT_NOT_ASCII_MSG: &str = "identifier name contains non-ASCII character(s)";
const RESTORE_DIR: &str = "./tests/tmp/restored";
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;
const MIN_CONFIRMATIONS: u8 = 1;

static INIT: Once = Once::new();

static MINER: Lazy<RwLock<Miner>> = Lazy::new(|| RwLock::new(Miner { no_mine_count: 0 }));

fn _bitcoin_cli() -> [String; 9] {
    [
        s!("-f"),
        s!("tests/docker-compose.yml"),
        s!("exec"),
        s!("-T"),
        s!("-u"),
        s!("blits"),
        s!("bitcoind"),
        s!("bitcoin-cli"),
        s!("-regtest"),
    ]
}

fn drain_wallet(wallet: &Wallet, online: &Online) {
    let rcv_wallet = get_test_wallet(false, None);
    test_drain_to_destroy(wallet, online, &rcv_wallet.get_address().unwrap());
}

fn fund_wallet(address: String) {
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

#[derive(Clone, Debug)]
struct Miner {
    no_mine_count: u32,
}

impl Miner {
    fn mine(&self) -> bool {
        if self.no_mine_count > 0 {
            return false;
        }
        let status = Command::new("docker")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .arg("compose")
            .args(_bitcoin_cli())
            .arg("-rpcwallet=miner")
            .arg("-generate")
            .arg("1")
            .status()
            .expect("failed to mine");
        assert!(status.success());
        wait_electrs_sync();
        true
    }

    fn stop_mining(&mut self) {
        self.no_mine_count += 1;
    }

    fn resume_mining(&mut self) {
        if self.no_mine_count > 0 {
            self.no_mine_count -= 1;
        }
    }
}

fn mine(resume: bool) {
    let t_0 = OffsetDateTime::now_utc();
    if resume {
        resume_mining();
    }
    let mut last_result = false;
    while !last_result {
        let miner = MINER.read();
        last_result = miner.as_ref().expect("MINER has been initialized").mine();
        drop(miner);
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 120.0 {
            println!("forcibly breaking mining wait");
            resume_mining();
        }
        if !last_result {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}

fn stop_mining() {
    MINER
        .write()
        .expect("MINER has been initialized")
        .stop_mining()
}

fn resume_mining() {
    MINER
        .write()
        .expect("MINER has been initialized")
        .resume_mining()
}

fn wait_electrs_sync() {
    let t_0 = OffsetDateTime::now_utc();
    let output = Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .arg("compose")
        .args(_bitcoin_cli())
        .arg("getblockcount")
        .output()
        .expect("failed to call getblockcount");
    assert!(output.status.success());
    let blockcount_str =
        std::str::from_utf8(&output.stdout).expect("could not parse blockcount output");
    let blockcount = blockcount_str
        .trim()
        .parse::<u32>()
        .expect("could not parte blockcount");
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut all_synced = true;
        for electrum_url in [ELECTRUM_URL, ELECTRUM_2_URL] {
            let electrum =
                electrum_client::Client::new(electrum_url).expect("cannot get electrum client");
            if electrum.block_header(blockcount as usize).is_err() {
                all_synced = false;
            }
        }
        if all_synced {
            break;
        };
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 10.0 {
            panic!("electrs not syncing with bitcoind");
        }
    }
}

pub fn initialize() {
    INIT.call_once(|| {
        println!("starting test services...");
        let status = Command::new("./tests/start_services.sh")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("failed to start test services");
        assert!(status.success());
    });
}

fn get_test_wallet_data(data_dir: &str, pubkey: &str, mnemonic: &str) -> WalletData {
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
fn get_test_wallet_with_net(
    private_keys: bool,
    max_allocations_per_utxo: Option<u32>,
    bitcoin_network: BitcoinNetwork,
) -> Wallet {
    let tests_data = TEST_DATA_DIR;
    fs::create_dir_all(tests_data).unwrap();

    let keys = generate_keys(bitcoin_network);
    let mut mnemonic = None;
    if private_keys {
        mnemonic = Some(keys.mnemonic)
    }
    Wallet::new(WalletData {
        data_dir: tests_data.to_string(),
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
fn get_test_wallet(private_keys: bool, max_allocations_per_utxo: Option<u32>) -> Wallet {
    get_test_wallet_with_net(
        private_keys,
        max_allocations_per_utxo,
        BitcoinNetwork::Regtest,
    )
}

// the get_*_wallet! macros can be called with no arguments to use defaults
fn get_empty_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys, None);
    if print_log {
        println!("wallet directory: {:?}", test_get_wallet_dir(&wallet));
    }
    let online = wallet.go_online(true, ELECTRUM_URL.to_string()).unwrap();
    (wallet, online)
}
macro_rules! get_empty_wallet {
    ($p: expr, $k: expr) => {
        get_empty_wallet($p, $k)
    };
    () => {
        get_empty_wallet(false, true)
    };
}

fn get_funded_noutxo_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let (wallet, online) = get_empty_wallet(print_log, private_keys);
    fund_wallet(wallet.get_address().unwrap());
    (wallet, online)
}
macro_rules! get_funded_noutxo_wallet {
    ($p: expr, $k: expr) => {
        get_funded_noutxo_wallet($p, $k)
    };
    () => {
        get_funded_noutxo_wallet(false, true)
    };
}

fn get_funded_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let (mut wallet, online) = get_funded_noutxo_wallet(print_log, private_keys);
    test_create_utxos_default(&mut wallet, &online);
    (wallet, online)
}
macro_rules! get_funded_wallet {
    ($p: expr, $k: expr) => {
        get_funded_wallet($p, $k)
    };
    () => {
        get_funded_wallet(false, true)
    };
}

fn test_blind_receive(wallet: &mut Wallet) -> ReceiveData {
    wallet
        .blind_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap()
}

fn test_witness_receive(wallet: &mut Wallet) -> ReceiveData {
    wallet
        .witness_receive(
            None,
            None,
            None,
            TRANSPORT_ENDPOINTS.clone(),
            MIN_CONFIRMATIONS,
        )
        .unwrap()
}

fn test_create_utxos_default(wallet: &mut Wallet, online: &Online) -> u8 {
    _test_create_utxos(wallet, online, false, None, None, FEE_RATE)
}

fn test_create_utxos(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> u8 {
    _test_create_utxos(wallet, online, up_to, num, size, fee_rate)
}

fn test_create_utxos_begin_result(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> Result<String, Error> {
    wallet.create_utxos_begin(online.clone(), up_to, num, size, fee_rate)
}

fn _test_create_utxos(
    wallet: &mut Wallet,
    online: &Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> u8 {
    let delay = 200;
    let mut retries = 3;
    let mut num_utxos_created = 0;
    while retries > 0 {
        retries -= 1;
        let result = wallet.create_utxos(online.clone(), up_to, num, size, fee_rate);
        match result {
            Ok(_) => {
                num_utxos_created = result.unwrap();
                break;
            }
            Err(Error::InsufficientBitcoins {
                needed: _,
                available: _,
            }) => {
                std::thread::sleep(Duration::from_millis(delay));
                continue;
            }
            Err(error) => {
                panic!("error creating UTXOs for wallet: {error:?}");
            }
        }
    }
    if num_utxos_created == 0 {
        panic!("error creating UTXOs for wallet: insufficient bitcoins");
    }
    num_utxos_created
}

fn test_delete_transfers(
    wallet: &Wallet,
    recipient_id: Option<&str>,
    txid: Option<&str>,
    no_asset_only: bool,
) -> bool {
    test_delete_transfers_result(wallet, recipient_id, txid, no_asset_only).unwrap()
}

fn test_delete_transfers_result(
    wallet: &Wallet,
    recipient_id: Option<&str>,
    txid: Option<&str>,
    no_asset_only: bool,
) -> Result<bool, Error> {
    let recipient_id = recipient_id.map(|id| id.to_string());
    let txid = txid.map(|id| id.to_string());
    wallet.delete_transfers(recipient_id, txid, no_asset_only)
}

fn test_drain_to_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    destroy_assets: bool,
) -> Result<String, Error> {
    wallet.drain_to(
        online.clone(),
        address.to_string(),
        destroy_assets,
        FEE_RATE,
    )
}

fn test_drain_to_begin_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    destroy_assets: bool,
    fee_rate: f32,
) -> Result<String, Error> {
    wallet.drain_to_begin(
        online.clone(),
        address.to_string(),
        destroy_assets,
        fee_rate,
    )
}

fn test_drain_to_destroy(wallet: &Wallet, online: &Online, address: &str) -> String {
    wallet
        .drain_to(online.clone(), address.to_string(), true, FEE_RATE)
        .unwrap()
}

fn test_drain_to_keep(wallet: &Wallet, online: &Online, address: &str) -> String {
    wallet
        .drain_to(online.clone(), address.to_string(), false, FEE_RATE)
        .unwrap()
}

fn test_fail_transfers_all(wallet: &mut Wallet, online: &Online) -> bool {
    wallet
        .fail_transfers(online.clone(), None, None, false)
        .unwrap()
}

fn test_fail_transfers_blind(wallet: &mut Wallet, online: &Online, blinded_utxo: &str) -> bool {
    wallet
        .fail_transfers(online.clone(), Some(blinded_utxo.to_string()), None, false)
        .unwrap()
}

fn test_fail_transfers_txid(wallet: &mut Wallet, online: &Online, txid: &str) -> bool {
    wallet
        .fail_transfers(online.clone(), None, Some(txid.to_string()), false)
        .unwrap()
}

fn test_get_address(wallet: &Wallet) -> String {
    wallet.get_address().unwrap()
}

fn test_get_asset_balance(wallet: &Wallet, asset_id: &str) -> Balance {
    test_get_asset_balance_result(wallet, asset_id).unwrap()
}

fn test_get_asset_balance_result(wallet: &Wallet, asset_id: &str) -> Result<Balance, Error> {
    wallet.get_asset_balance(asset_id.to_string())
}

fn test_get_asset_metadata(wallet: &mut Wallet, asset_id: &str) -> Metadata {
    test_get_asset_metadata_result(wallet, asset_id).unwrap()
}

fn test_get_asset_metadata_result(wallet: &mut Wallet, asset_id: &str) -> Result<Metadata, Error> {
    wallet.get_asset_metadata(asset_id.to_string())
}

fn test_get_btc_balance(wallet: &Wallet, online: &Online) -> BtcBalance {
    wallet.get_btc_balance(online.clone()).unwrap()
}

fn test_get_wallet_data(wallet: &Wallet) -> WalletData {
    wallet.get_wallet_data()
}

fn test_get_wallet_dir(wallet: &Wallet) -> PathBuf {
    wallet.get_wallet_dir()
}

fn test_go_online(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    electrum_url: Option<&str>,
) -> Online {
    test_go_online_result(wallet, skip_consistency_check, electrum_url).unwrap()
}

fn test_go_online_result(
    wallet: &mut Wallet,
    skip_consistency_check: bool,
    electrum_url: Option<&str>,
) -> Result<Online, Error> {
    let electrum = electrum_url.unwrap_or(ELECTRUM_URL).to_string();
    wallet.go_online(skip_consistency_check, electrum)
}

fn test_issue_asset_cfa(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> AssetCFA {
    test_issue_asset_cfa_result(wallet, online, amounts, file_path).unwrap()
}

fn test_issue_asset_cfa_result(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
    file_path: Option<String>,
) -> Result<AssetCFA, Error> {
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_cfa(
        online.clone(),
        NAME.to_string(),
        Some(DESCRIPTION.to_string()),
        PRECISION,
        amounts,
        file_path,
    )
}

fn test_issue_asset_nia(wallet: &mut Wallet, online: &Online, amounts: Option<&[u64]>) -> AssetNIA {
    test_issue_asset_nia_result(wallet, online, amounts).unwrap()
}

fn test_issue_asset_nia_result(
    wallet: &mut Wallet,
    online: &Online,
    amounts: Option<&[u64]>,
) -> Result<AssetNIA, Error> {
    let amounts = if let Some(a) = amounts {
        a.to_vec()
    } else {
        vec![AMOUNT]
    };
    wallet.issue_asset_nia(
        online.clone(),
        TICKER.to_string(),
        NAME.to_string(),
        PRECISION,
        amounts,
    )
}

fn test_list_assets(wallet: &mut Wallet, filter_asset_schemas: &[AssetSchema]) -> Assets {
    wallet.list_assets(filter_asset_schemas.to_vec()).unwrap()
}

fn test_list_transactions(wallet: &Wallet, online: Option<&Online>) -> Vec<Transaction> {
    let online = online.cloned();
    wallet.list_transactions(online).unwrap()
}

fn test_list_transfers(wallet: &Wallet, asset_id: Option<&str>) -> Vec<Transfer> {
    test_list_transfers_result(wallet, asset_id).unwrap()
}

fn test_list_transfers_result(
    wallet: &Wallet,
    asset_id: Option<&str>,
) -> Result<Vec<Transfer>, Error> {
    let asset_id = asset_id.map(|a| a.to_string());
    wallet.list_transfers(asset_id)
}

fn test_list_unspents(
    wallet: &Wallet,
    online: Option<&Online>,
    settled_only: bool,
) -> Vec<Unspent> {
    let online = online.cloned();
    wallet.list_unspents(online, settled_only).unwrap()
}

fn test_refresh_all(wallet: &mut Wallet, online: &Online) -> bool {
    wallet.refresh(online.clone(), None, vec![]).unwrap()
}

fn test_refresh_asset(wallet: &mut Wallet, online: &Online, asset_id: &str) -> bool {
    wallet
        .refresh(online.clone(), Some(asset_id.to_string()), vec![])
        .unwrap()
}

fn test_send(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> String {
    test_send_result(wallet, online, recipient_map).unwrap()
}

fn test_send_result(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<String, Error> {
    wallet.send(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

fn test_send_begin_result(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: &HashMap<String, Vec<Recipient>>,
) -> Result<String, Error> {
    wallet.send_begin(
        online.clone(),
        recipient_map.clone(),
        false,
        FEE_RATE,
        MIN_CONFIRMATIONS,
    )
}

fn test_send_btc(wallet: &Wallet, online: &Online, address: &str, amount: u64) -> String {
    test_send_btc_result(wallet, online, address, amount).unwrap()
}

fn test_send_btc_result(
    wallet: &Wallet,
    online: &Online,
    address: &str,
    amount: u64,
) -> Result<String, Error> {
    wallet.send_btc(online.clone(), address.to_string(), amount, FEE_RATE)
}

fn check_test_transfer_status_recipient(
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

fn check_test_transfer_status_sender(
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

fn check_test_wallet_data(
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

fn compare_test_directories(src: &Path, dst: &Path, skip: &[&str]) {
    let ignores = RegexSet::new(skip).unwrap();
    let cmp = dircmp::Comparison::new(ignores);
    let diff = cmp.compare(src, dst).unwrap();
    assert!(diff.is_empty());
}

fn get_test_batch_transfers(wallet: &Wallet, txid: &str) -> Vec<DbBatchTransfer> {
    wallet
        .database
        .iter_batch_transfers()
        .unwrap()
        .into_iter()
        .filter(|b| b.txid == Some(txid.to_string()))
        .collect()
}

fn get_test_asset_transfers(wallet: &Wallet, batch_transfer_idx: i32) -> Vec<DbAssetTransfer> {
    wallet
        .database
        .iter_asset_transfers()
        .unwrap()
        .into_iter()
        .filter(|at| at.batch_transfer_idx == batch_transfer_idx)
        .collect()
}

fn get_test_transfers(wallet: &Wallet, asset_transfer_idx: i32) -> Vec<DbTransfer> {
    wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .filter(|t| t.asset_transfer_idx == asset_transfer_idx)
        .collect()
}

fn get_test_asset_transfer(wallet: &Wallet, batch_transfer_idx: i32) -> DbAssetTransfer {
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer_idx);
    assert_eq!(asset_transfers.len(), 1);
    asset_transfers.first().unwrap().clone()
}

fn get_test_coloring(wallet: &Wallet, asset_transfer_idx: i32) -> DbColoring {
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

fn get_test_transfer_recipient(wallet: &Wallet, recipient_id: &str) -> DbTransfer {
    wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .find(|t| t.recipient_id == Some(recipient_id.to_string()))
        .unwrap()
}

fn get_test_transfer_sender(
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

fn get_test_transfers_sender(
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

fn get_test_transfer_data(
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

fn get_test_transfer_related(
    wallet: &Wallet,
    transfer: &DbTransfer,
) -> (DbAssetTransfer, DbBatchTransfer) {
    let db_data = wallet.database.get_db_data(false).unwrap();
    transfer
        .related_transfers(&db_data.asset_transfers, &db_data.batch_transfers)
        .unwrap()
}

fn get_test_txo(wallet: &Wallet, idx: i32) -> DbTxo {
    wallet
        .database
        .iter_txos()
        .unwrap()
        .into_iter()
        .find(|t| t.idx == idx)
        .unwrap()
}

fn list_test_unspents(wallet: &Wallet, msg: &str) -> Vec<Unspent> {
    let unspents = test_list_unspents(wallet, None, false);
    println!(
        "unspents for wallet {:?} {}: {}",
        test_get_wallet_dir(wallet),
        msg,
        unspents.len()
    );
    for unspent in &unspents {
        println!("- {unspent:?}");
    }
    unspents
}

fn wait_for_unspent_num(wallet: &Wallet, online: Online, num_unspents: usize) {
    let t_0 = OffsetDateTime::now_utc();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let unspents = test_list_unspents(wallet, Some(&online), false);
        if unspents.len() >= num_unspents {
            break;
        };
        if (OffsetDateTime::now_utc() - t_0).as_seconds_f32() > 10.0 {
            panic!("cannot find funding UTXO");
        }
    }
}

/// print the provided message, then get colorings for each wallet unspent and print their status,
/// type, amount and asset
fn show_unspent_colorings(wallet: &Wallet, msg: &str) {
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
                db_coloring.coloring_type,
                db_coloring.amount,
                db_asset_transfer.asset_id.as_ref(),
            );
        }
    }
}

lazy_static! {
    static ref MOCK_CONTRACT_DATA: Mutex<Vec<Attachment>> = Mutex::new(vec![]);
}

pub fn mock_contract_data(terms: RicardianContract, media: Option<Attachment>) -> ContractData {
    let mut mock_reqs = MOCK_CONTRACT_DATA.lock().unwrap();
    if mock_reqs.is_empty() {
        ContractData { terms, media }
    } else {
        let mocked_media = mock_reqs.pop();
        ContractData {
            terms,
            media: mocked_media,
        }
    }
}

mod backup;
mod blind_receive;
mod create_utxos;
mod delete_transfers;
mod drain_to;
mod fail_transfers;
mod get_address;
mod get_asset_balance;
mod get_asset_metadata;
mod get_btc_balance;
mod go_online;
mod issue_asset_cfa;
mod issue_asset_nia;
mod list_assets;
mod list_transactions;
mod list_transfers;
mod list_unspents;
mod new;
mod refresh;
mod send;
mod send_btc;
mod witness_receive;
