use amplify::confinement::Confined;
use amplify::{s, Wrapper};
use bdk::bitcoin::bip32::ExtendedPubKey;
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::{psbt::Psbt as BdkPsbt, Network as BdkNetwork};
use bdk::keys::ExtendedKey;
use bdk::{KeychainKind, LocalUtxo, SignOptions};
use bitcoin::hashes::{sha256, Hash as Sha256Hash};
use bp::Outpoint as RgbOutpoint;
#[cfg(feature = "electrum")]
use electrum_client::{ElectrumApi, Param};
use futures::executor::block_on;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use regex::RegexSet;
use rgbstd::containers::FileContent;
use rgbstd::containers::Transfer as RgbTransfer;
use rgbstd::contract::ContractId;
use rgbstd::interface::rgb21::{TokenData, TokenIndex};
use rgbstd::invoice::ChainNet;
use rgbstd::stl::{AssetTerms, Attachment, Details, MediaType, Name, RicardianContract, Ticker};
use sea_orm::ActiveValue;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::MAIN_SEPARATOR;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::{Mutex, Once, RwLock};
use std::time::Duration;
use time::OffsetDateTime;

#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::api::proxy::Proxy;
use crate::database::entities::asset::ActiveModel as DbAssetActMod;
use crate::database::entities::asset_transfer::Model as DbAssetTransfer;
use crate::database::entities::batch_transfer::{
    ActiveModel as DbBatchTransferActMod, Model as DbBatchTransfer,
};
use crate::database::entities::coloring::Model as DbColoring;
use crate::database::entities::transfer::Model as DbTransfer;
use crate::database::entities::txo::Model as DbTxo;
use crate::database::{LocalUnspent, TransferData};
use crate::utils::now;
use crate::wallet::offline::*;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::wallet::online::*;
#[cfg(any(feature = "electrum", feature = "esplora"))]
use crate::wallet::online::{BtcBalance, Indexer};
use crate::*;

use utils::api::*;
use utils::chain::*;
use utils::helpers::*;

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
const ELECTRUM_BLOCKSTREAM_URL: &str = "127.0.0.1:50003";
const ESPLORA_URL: &str = "http://127.0.0.1:8094/regtest/api";
const TEST_DATA_DIR_PARTS: [&str; 2] = ["tests", "tmp"];
const TICKER: &str = "TICKER";
const NAME: &str = "asset name";
const DETAILS: &str = "details with ℧nicode characters";
const PRECISION: u8 = 7;
const AMOUNT: u64 = 666;
const FEE_RATE: f32 = 1.5;
const FEE_MSG_LOW: &str = "value under minimum 1";
const FEE_MSG_HIGH: &str = "value above maximum 1000";
const IDENT_EMPTY_MSG: &str = "ident must contain at least one character";
const IDENT_TOO_LONG_MSG: &str = "identifier name has invalid length";
const IDENT_NOT_ASCII_MSG: &str = "identifier name contains non-ASCII character(s)";
const RESTORE_DIR_PARTS: [&str; 3] = ["tests", "tmp", "restored"];
const MAX_ALLOCATIONS_PER_UTXO: u32 = 5;
const MIN_CONFIRMATIONS: u8 = 1;
const FAKE_TXID: &str = "e5a3e577309df31bd606f48049049d2e1e02b048206ba232944fcc053a176ccb:0";
const UNKNOWN_IDX: i32 = 9999;

static INIT: Once = Once::new();

#[derive(Debug, Deserialize)]
struct Block {
    tx: Vec<String>,
}

pub fn get_regtest_txid() -> String {
    let bestblockhash = Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .arg("compose")
        .args(bitcoin_cli())
        .arg("getbestblockhash")
        .output()
        .expect("failed to call getblockcount");
    assert!(bestblockhash.status.success());
    let bestblockhash_str = std::str::from_utf8(&bestblockhash.stdout)
        .expect("could not parse bestblockhash output")
        .trim();
    let block = Command::new("docker")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .arg("compose")
        .args(bitcoin_cli())
        .arg("getblock")
        .arg(bestblockhash_str)
        .output()
        .expect("failed to call getblockcount");
    assert!(block.status.success());
    let block_str =
        std::str::from_utf8(&block.stdout).expect("could not parse bestblockhash output");
    let block: Block = serde_json::from_str(block_str).expect("failed to deserialize block JSON");
    assert!(!block.tx.is_empty());
    block.tx.first().unwrap().clone()
}

pub fn initialize() {
    let start_services_file = ["tests", "start_services.sh"].join(&MAIN_SEPARATOR.to_string());
    INIT.call_once(|| {
        println!("starting test services...");
        let status = Command::new(start_services_file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("failed to start test services");
        assert!(status.success());
    });
}

// the get_*_wallet! macros can be called with no arguments to use defaults
#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_empty_wallet {
    ($i: expr) => {
        get_funded_wallet(true, Some($i))
    };
    () => {
        get_empty_wallet(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_funded_noutxo_wallet {
    ($i: expr) => {
        get_funded_wallet(true, Some($i))
    };
    () => {
        get_funded_noutxo_wallet(true, None)
    };
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
macro_rules! get_funded_wallet {
    ($i: expr) => {
        get_funded_wallet(true, Some($i))
    };
    () => {
        get_funded_wallet(true, None)
    };
}

lazy_static! {
    static ref MOCK_CONTRACT_DATA: Mutex<Vec<Attachment>> = Mutex::new(vec![]);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_asset_terms(
    wallet: &Wallet,
    text: RicardianContract,
    media: Option<Attachment>,
) -> AssetTerms {
    let mut mock_reqs = MOCK_CONTRACT_DATA.lock().unwrap();
    if mock_reqs.is_empty() {
        wallet.new_asset_terms(text, media)
    } else {
        let mocked_media = mock_reqs.pop();
        wallet.new_asset_terms(text, mocked_media)
    }
}

lazy_static! {
    static ref MOCK_TOKEN_DATA: Mutex<Vec<TokenData>> = Mutex::new(vec![]);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_token_data(
    wallet: &Wallet,
    index: TokenIndex,
    media_data: &Option<(Attachment, Media)>,
    attachments: BTreeMap<u8, Attachment>,
) -> TokenData {
    let mut mock_reqs = MOCK_TOKEN_DATA.lock().unwrap();
    if mock_reqs.is_empty() {
        wallet.new_token_data(index, media_data, attachments)
    } else {
        mock_reqs.pop().unwrap()
    }
}

lazy_static! {
    static ref MOCK_INPUT_UNSPENTS: Mutex<Vec<LocalUnspent>> = Mutex::new(vec![]);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_input_unspents(wallet: &Wallet, unspents: &[LocalUnspent]) -> Vec<LocalUnspent> {
    let mut mock_input_unspents = MOCK_INPUT_UNSPENTS.lock().unwrap();
    if mock_input_unspents.is_empty() {
        wallet.get_input_unspents(unspents).unwrap()
    } else {
        mock_input_unspents.drain(..).collect()
    }
}

lazy_static! {
    static ref MOCK_CONTRACT_DETAILS: Mutex<Option<&'static str>> = Mutex::new(None);
}

#[cfg(any(feature = "electrum", feature = "esplora"))]
pub fn mock_contract_details(wallet: &Wallet) -> Option<Details> {
    MOCK_CONTRACT_DETAILS
        .lock()
        .unwrap()
        .take()
        .map(|d| wallet.check_details(d.to_string()).unwrap())
}

lazy_static! {
    static ref MOCK_CHAIN_NET: Mutex<Option<ChainNet>> = Mutex::new(None);
}

pub fn mock_chain_net(wallet: &Wallet) -> ChainNet {
    match MOCK_CHAIN_NET.lock().unwrap().take() {
        Some(chain_net) => chain_net,
        None => wallet.bitcoin_network().into(),
    }
}

// test utilities
mod utils;

// API tests
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
mod get_wallet_data;
mod get_wallet_dir;
mod go_online;
mod issue_asset_cfa;
mod issue_asset_nia;
mod issue_asset_uda;
mod list_assets;
mod list_transactions;
mod list_transfers;
mod list_unspents;
mod list_unspents_vanilla;
mod new;
mod refresh;
mod save_new_asset;
mod send;
mod send_btc;
mod sign_psbt;
mod witness_receive;
