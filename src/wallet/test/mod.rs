use amplify::s;
use once_cell::sync::Lazy;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Once, RwLock};
use time::OffsetDateTime;
use walkdir::WalkDir;

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

fn drain_wallet(wallet: &Wallet, online: Online) {
    let rcv_wallet = get_test_wallet(false, None);
    wallet
        .drain_to(online, rcv_wallet.get_address(), true, FEE_RATE)
        .unwrap();
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

// return a regtest wallet for testing.
fn get_test_wallet(private_keys: bool, max_allocations_per_utxo: Option<u32>) -> Wallet {
    let tests_data = TEST_DATA_DIR;
    fs::create_dir_all(tests_data).unwrap();

    let bitcoin_network = BitcoinNetwork::Regtest;
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
    })
    .unwrap()
}

// the get_*_wallet! macros can be called with no arguments to use defaults
fn get_empty_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys, None);
    if print_log {
        println!("wallet directory: {:?}", wallet.get_wallet_dir());
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
    fund_wallet(wallet.get_address());
    mine(false);
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
    test_create_utxos_default(&mut wallet, online.clone());
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

fn test_create_utxos_default(wallet: &mut Wallet, online: Online) -> u8 {
    _test_create_utxos(wallet, online, false, None, None, FEE_RATE)
}

fn test_create_utxos(
    wallet: &mut Wallet,
    online: Online,
    up_to: bool,
    num: Option<u8>,
    size: Option<u32>,
    fee_rate: f32,
) -> u8 {
    _test_create_utxos(wallet, online, up_to, num, size, fee_rate)
}

fn _test_create_utxos(
    wallet: &mut Wallet,
    online: Online,
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

fn test_send_default(
    wallet: &mut Wallet,
    online: &Online,
    recipient_map: HashMap<String, Vec<Recipient>>,
) -> String {
    wallet
        .send(
            online.clone(),
            recipient_map,
            false,
            FEE_RATE,
            MIN_CONFIRMATIONS,
        )
        .unwrap()
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
    asset: &AssetRgb20,
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
    let assets = wallet.list_assets(vec![]).unwrap();
    let rgb20_assets = assets.rgb20.unwrap();
    let rgb25_assets = assets.rgb25.unwrap();
    assert_eq!(rgb20_assets.len(), 1);
    assert_eq!(rgb25_assets.len(), 0);
    let rgb20_asset = rgb20_assets.first().unwrap();
    assert_eq!(rgb20_asset.asset_id, asset.asset_id);
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
    let metadata = wallet.get_asset_metadata(asset.asset_id.clone()).unwrap();
    assert_eq!(metadata.asset_iface, AssetIface::RGB20);
    assert_eq!(metadata.issued_supply, issued_supply);
    assert_eq!(metadata.name, asset.name);
    assert_eq!(metadata.precision, asset.precision);
    assert_eq!(metadata.ticker.unwrap(), asset.ticker);
    // transfer list
    let transfers = wallet.list_transfers(asset.asset_id.clone()).unwrap();
    assert_eq!(transfers.len(), 1 + transfer_num);
    assert_eq!(transfers.first().unwrap().kind, TransferKind::Issuance);
    assert_eq!(transfers.last().unwrap().kind, TransferKind::Send);
    assert_eq!(transfers.last().unwrap().status, TransferStatus::Settled);
    // unspent list
    let unspents = wallet.list_unspents(None, false).unwrap();
    assert_eq!(unspents.len(), 6);
}

fn compare_test_directories(src: &Path, dst: &Path, skip_src: Vec<&str>) -> (bool, String) {
    const BUF_SIZE: usize = 4096;
    let mut walk_src = WalkDir::new(src)
        .sort_by(|a, b| a.path().cmp(b.path()))
        .into_iter();
    let mut walk_dst = WalkDir::new(dst)
        .sort_by(|a, b| a.path().cmp(b.path()))
        .into_iter();
    let (same, msg) = loop {
        let path_src = walk_src.next();
        if path_src.is_some() {
            let file_name = path_src
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .path()
                .strip_prefix(src)
                .unwrap()
                .file_name();
            if let Some(name) = file_name {
                if skip_src.contains(&name.to_str().unwrap()) {
                    continue;
                }
            }
        }
        let path_dst = walk_dst.next();
        if path_src.is_none() && path_dst.is_none() {
            break (true, s!(""));
        }

        let path_src = path_src
            .unwrap()
            .unwrap_or_else(|e| panic!("error walking original directory: {e}"));
        let path_dst = path_dst
            .unwrap()
            .unwrap_or_else(|e| panic!("error walking restored directory: {e}"));
        let path_src = path_src.path();
        let path_dst = path_dst.path();
        let path_src_str = path_src
            .to_str()
            .ok_or_else(|| panic!("error getting original file path string"))
            .unwrap();
        let path_dst_str = path_dst
            .to_str()
            .ok_or_else(|| panic!("error getting restored file path string"))
            .unwrap();
        if path_src.strip_prefix(src) != path_dst.strip_prefix(dst) {
            break (false, s!("original and restored file paths differ: \"{path_src_str}\" != \"{path_dst_str}\""));
        }
        if path_src.is_dir() && path_src.is_dir() {
            continue;
        }
        let file_src = std::fs::File::open(path_src);
        let file_dst = std::fs::File::open(path_dst);
        let file_src = file_src
            .unwrap_or_else(|e| panic!("error opening original file \"{path_src_str}\": {e}"));
        let file_dst = file_dst
            .unwrap_or_else(|e| panic!("error opening restored file \"{path_dst_str}\": {e}"));
        let mut read_src = std::io::BufReader::new(file_src);
        let mut read_dst = std::io::BufReader::new(file_dst);
        let mut buf_src = [0; BUF_SIZE];
        let mut buf_dst = [0; BUF_SIZE];
        let same = loop {
            let bytes_read_src = read_src
                .read(&mut buf_src)
                .unwrap_or_else(|e| panic!("error reading from file \"{path_src_str}\": {e}"));
            let bytes_read_dst = read_dst
                .read(&mut buf_dst)
                .unwrap_or_else(|e| panic!("error reading from file \"{path_dst_str}\": {e}"));
            if bytes_read_src == 0 && bytes_read_dst == 0 {
                break true;
            }
            if bytes_read_src != bytes_read_dst || buf_src != buf_dst {
                break false;
            } else {
                continue;
            }
        };
        if same {
            continue;
        } else {
            break (
                false,
                s!("differing files found: \"{path_src_str}\" != \"{path_dst_str}\""),
            );
        }
    };
    (same, msg)
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
    let unspents = wallet.list_unspents(None, false).unwrap();
    println!(
        "unspents for wallet {:?} {}: {}",
        wallet.get_wallet_dir(),
        msg,
        unspents.len()
    );
    for unspent in &unspents {
        println!("- {unspent:?}");
    }
    unspents
}

/// print the provided message, then get colorings for each wallet unspent and print their status,
/// type, amount and asset
fn show_unspent_colorings(wallet: &Wallet, msg: &str) {
    println!("\n{msg}");
    let unspents = wallet.list_unspents(None, false).unwrap();
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
            "> {}:{}, {} sat, {}colorable",
            outpoint.txid,
            outpoint.vout,
            unspent.utxo.btc_amount,
            if unspent.utxo.colorable { "" } else { "not " }
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

mod backup;
mod blind_receive;
mod create_utxos;
mod delete_transfers;
mod drain_to;
mod fail_transfers;
mod get_address;
mod get_asset_balance;
mod get_asset_metadata;
mod go_online;
mod issue_asset_rgb20;
mod issue_asset_rgb25;
mod list_assets;
mod list_transactions;
mod list_transfers;
mod list_unspents;
mod new;
mod refresh;
mod send;
mod witness_receive;
