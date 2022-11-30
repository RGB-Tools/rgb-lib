use amplify::s;
use std::process::{Command, Stdio};
use std::sync::Once;

use crate::generate_keys;

use super::*;

const PROXY_URL: &str = "http://proxy.rgbtools.org";
const ELECTRUM_URL: &str = "127.0.0.1:50001";
const TEST_DATA_DIR: &str = "./tests/tmp";
const TICKER: &str = "TICKER";
const NAME: &str = "name";
const DESCRIPTION: &str = "DESCRIPTION";
const PRECISION: u8 = 7;
const AMOUNT: u64 = 666;

static INIT: Once = Once::new();

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

fn fund_wallet(address: String) {
    let status = Command::new("docker-compose")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(_bitcoin_cli())
        .arg("-rpcwallet=miner")
        .arg("sendtoaddress")
        .arg(address)
        .arg("1")
        .status()
        .expect("failed to fund wallet");
    assert!(status.success());
}

fn mine() {
    let status = Command::new("docker-compose")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args(_bitcoin_cli())
        .arg("-rpcwallet=miner")
        .arg("-generate")
        .arg("3")
        .status()
        .expect("failed to mine");
    assert!(status.success());
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
fn get_test_wallet(private_keys: bool) -> Wallet {
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
        pubkey: keys.xpub,
        mnemonic,
    })
    .unwrap()
}

// the get_*_wallet! macros can be called with no arguments to use defaults
fn get_empty_wallet(print_log: bool, private_keys: bool) -> (Wallet, Online) {
    let mut wallet = get_test_wallet(private_keys);
    if print_log {
        println!("wallet directory: {:?}", wallet.get_wallet_dir());
    }
    let online = wallet
        .go_online(true, ELECTRUM_URL.to_string(), PROXY_URL.to_string())
        .unwrap();
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
    mine();
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
    wallet
        .create_utxos(online.clone(), false, None, None)
        .unwrap();
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

fn check_test_transfer_status_recipient(
    wallet: &Wallet,
    blinded_utxo: &str,
    expected_status: TransferStatus,
) -> bool {
    let transfers = wallet.database.iter_transfers().unwrap();
    let transfer = transfers
        .iter()
        .find(|t| t.blinded_utxo == Some(blinded_utxo.to_string()))
        .unwrap();
    let transfer_data = wallet.database.get_transfer_data(transfer).unwrap();
    println!(
        "receive with blinded_utxo {} is in status {:?}",
        blinded_utxo, &transfer_data.status
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

fn get_test_batch_transfers(wallet: &Wallet, txid: &str) -> Vec<DbBatchTransfer> {
    wallet
        .database
        .iter_batch_transfers()
        .unwrap()
        .into_iter()
        .filter(|b| b.txid == Some(txid.to_string()))
        .collect()
}

fn get_test_asset_transfers(wallet: &Wallet, batch_transfer_idx: i64) -> Vec<DbAssetTransfer> {
    wallet
        .database
        .iter_asset_transfers()
        .unwrap()
        .into_iter()
        .filter(|at| at.batch_transfer_idx == batch_transfer_idx)
        .collect()
}

fn get_test_transfers(wallet: &Wallet, asset_transfer_idx: i64) -> Vec<DbTransfer> {
    wallet
        .database
        .iter_transfers()
        .unwrap()
        .into_iter()
        .filter(|t| t.asset_transfer_idx == asset_transfer_idx)
        .collect()
}

fn get_test_asset_transfer(wallet: &Wallet, batch_transfer_idx: i64) -> DbAssetTransfer {
    let asset_transfers = get_test_asset_transfers(wallet, batch_transfer_idx);
    assert_eq!(asset_transfers.len(), 1);
    asset_transfers.first().unwrap().clone()
}

fn get_test_coloring(wallet: &Wallet, asset_transfer_idx: i64) -> DbColoring {
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

fn get_test_transfer_recipient(wallet: &Wallet, blinded_utxo: &str) -> DbTransfer {
    wallet
        .database
        .get_transfer(blinded_utxo.to_string())
        .unwrap()
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
        let asset_id = if asset_transfer.asset_rgb20_id.is_some() {
            asset_transfer.asset_rgb20_id
        } else {
            asset_transfer.asset_rgb121_id
        }
        .unwrap();
        let transfers_for_asset = get_test_transfers(wallet, asset_transfer.idx);
        transfers.insert(asset_id, transfers_for_asset);
    }
    (transfers.clone(), asset_transfers, batch_transfer.clone())
}

fn get_test_txo(wallet: &Wallet, idx: i64) -> DbTxo {
    wallet
        .database
        .iter_txos()
        .unwrap()
        .into_iter()
        .find(|t| t.idx == idx)
        .unwrap()
}

fn list_test_unspents(wallet: &Wallet, msg: &str) -> Vec<Unspent> {
    let unspents = wallet.list_unspents(false).unwrap();
    println!(
        "unspents for wallet {:?} {}: {}",
        wallet.get_wallet_dir(),
        msg,
        unspents.len()
    );
    for unspent in &unspents {
        println!("- {:?}", unspent);
    }
    unspents
}

/// print the provided message, then get colorings for each wallet unspent and print their status,
/// type, amount and asset
fn show_unspent_colorings(wallet: &Wallet, msg: &str) {
    println!("\n{}", msg);
    let unspents = wallet.list_unspents(false).unwrap();
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
                if db_asset_transfer.asset_rgb20_id.is_some() {
                    &db_asset_transfer.asset_rgb20_id
                } else {
                    &db_asset_transfer.asset_rgb121_id
                },
            );
        }
    }
}

mod blind;
mod create_utxos;
mod delete_transfers;
mod drain_to;
mod fail_transfers;
mod get_address;
mod get_asset_balance;
mod get_asset_metadata;
mod go_online;
mod issue_asset_rgb121;
mod issue_asset_rgb20;
mod list_assets;
mod list_transfers;
mod list_unspents;
mod new;
mod refresh;
mod send;
